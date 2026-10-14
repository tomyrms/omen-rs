//! The control service: one worker thread owns the BIOS WMI client and the
//! sensors, runs the automatic mode, and applies what the user asks for.
//! A second thread drives the keyboard (HID writes take a few hundred ms).
//!
//! The interface talks to it through [`Controller`]: commands go in through a
//! channel, the current state comes out as a [`Snapshot`].

use std::collections::VecDeque;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use omen_kbd::{EffectSettings, Keyboard, Rgb};
use omen_wmi::{
    AdapterStatus, BiosClient, FanCurve, FanTable, GpuMode, GpuPowerData, Sensor, SystemData,
    Throttling,
};

use crate::auto::{AutoEngine, Inputs};
use crate::config::{log_line, AutoSettings, Config, KeyboardLook};
use crate::games;
use crate::mode::{Level, Mode};
use crate::sensors::{self, GpuSample, Nvml, PdhSensors, PowerState};

/// One point of the history graph.
#[derive(Debug, Clone, Copy)]
pub struct HistoryPoint {
    pub t: f64,
    pub cpu_load: f32,
    pub gpu_temp: Option<f32>,
    pub fan_cpu_rpm: Option<u32>,
    pub fan_gpu_rpm: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct KeyboardState {
    pub available: Option<bool>,
    pub model: String,
    pub firmware: String,
    pub busy: bool,
    pub message: String,
    pub error: bool,
}

/// Everything the interface shows.
#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub elevated: bool,
    pub connected: bool,
    pub connect_error: Option<String>,
    pub system: Option<SystemData>,
    pub fan_count: usize,
    pub fan_cpu: Option<u8>,
    pub fan_gpu: Option<u8>,
    pub max_fan: Option<bool>,
    pub temp_ir: Option<u8>,
    pub temp_ambient: Option<u8>,
    pub temp_pch: Option<u8>,
    pub temp_vr: Option<u8>,
    pub throttling: bool,
    pub cpu_load: Option<f32>,
    pub gpu_load: Option<f32>,
    pub thermal_zone: Option<f32>,
    pub gpu: GpuSample,
    pub gpu_name: Option<String>,
    pub gpu_sensors_paused: bool,
    pub power: PowerState,
    pub adapter: Option<AdapterStatus>,
    pub gpu_power: Option<GpuPowerData>,
    pub gpu_mode: Option<GpuMode>,
    pub fan_table: Option<FanTable>,
    pub mode: Mode,
    pub applied: Option<Level>,
    pub auto_reason: String,
    pub game: Option<String>,
    pub vendor_app_running: bool,
    pub manual_fan: Option<u8>,
    pub custom_levels: Option<(u8, u8)>,
    pub history: VecDeque<HistoryPoint>,
    /// Result of the last user action (shown as a status line).
    pub message: String,
    pub message_is_error: bool,
    pub keyboard: KeyboardState,
}

impl Snapshot {
    pub fn fan_rpm(level: Option<u8>) -> Option<u32> {
        level.map(omen_wmi::level_to_rpm)
    }

    /// Best available "CPU side" temperature.
    pub fn cpu_temp(&self) -> Option<f32> {
        self.thermal_zone
            .filter(|t| *t > 1.0)
            .or(self.temp_pch.map(f32::from))
    }
}

pub enum Command {
    SetMode(Mode),
    SetAutoSettings(AutoSettings),
    SetCustomCurve(FanCurve),
    /// Manual fan level (raw units) for both fans; `None` returns control.
    SetManualFan(Option<u8>),
    SetGpuSensorsOnBattery(bool),
    SetGpuMux(GpuMode),
    SetGpuPower { custom_tgp: bool, ppab: bool },
    Keyboard(KeyboardCommand),
    RefreshInfo,
    /// Return fan control to the BIOS before the app exits.
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum KeyboardCommand {
    Apply(KeyboardLook),
    RestoreDefault,
    Detect,
}

pub struct Controller {
    tx: Sender<Command>,
    kbd_tx: Sender<KeyboardCommand>,
    state: Arc<Mutex<Snapshot>>,
}

fn lock(s: &Arc<Mutex<Snapshot>>) -> MutexGuard<'_, Snapshot> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

impl Controller {
    pub fn start(config: Config) -> Self {
        let (tx, rx) = channel();
        let (kbd_tx, kbd_rx) = channel();
        let state = Arc::new(Mutex::new(Snapshot {
            elevated: is_elevated(),
            mode: config.mode,
            ..Default::default()
        }));
        {
            let state = state.clone();
            let cfg = config.clone();
            std::thread::Builder::new()
                .name("omen-control".into())
                .spawn(move || Worker::new(state, cfg).run(rx))
                .expect("spawn control thread");
        }
        {
            let state = state.clone();
            std::thread::Builder::new()
                .name("omen-keyboard".into())
                .spawn(move || keyboard_thread(state, kbd_rx))
                .expect("spawn keyboard thread");
        }
        let _ = kbd_tx.send(KeyboardCommand::Detect);
        if config.restore_keyboard_on_start {
            let _ = kbd_tx.send(KeyboardCommand::Apply(config.keyboard.clone()));
        }
        Self { tx, kbd_tx, state }
    }

    pub fn send(&self, cmd: Command) {
        match cmd {
            Command::Keyboard(k) => {
                lock(&self.state).keyboard.busy = true;
                let _ = self.kbd_tx.send(k);
            }
            other => {
                let _ = self.tx.send(other);
            }
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        lock(&self.state).clone()
    }

    /// Lightweight read of a few fields without cloning the history.
    pub fn with<R>(&self, f: impl FnOnce(&Snapshot) -> R) -> R {
        f(&lock(&self.state))
    }

    /// Asks the worker to release fan control and waits briefly for it.
    pub fn shutdown(&self) {
        let _ = self.tx.send(Command::Shutdown);
        std::thread::sleep(Duration::from_millis(400));
    }
}

// ---------------------------------------------------------------------------
// Keyboard thread
// ---------------------------------------------------------------------------

fn scale(c: Rgb, brightness: u8) -> Rgb {
    let k = u16::from(brightness.min(100));
    let f = |v: u8| ((u16::from(v) * k + 50) / 100) as u8;
    Rgb::new(f(c.r), f(c.g), f(c.b))
}

fn keyboard_thread(state: Arc<Mutex<Snapshot>>, rx: Receiver<KeyboardCommand>) {
    let mut kb: Option<Keyboard> = None;
    while let Ok(cmd) = rx.recv() {
        if kb.is_none() {
            kb = Keyboard::open().ok();
        }
        let result: Result<String, String> = match (&mut kb, cmd) {
            (None, _) => Err("Clavier RGB non détecté sur ce PC.".into()),
            (Some(k), KeyboardCommand::Detect) => match k.device_info() {
                Ok(info) => {
                    let mut st = lock(&state);
                    st.keyboard.model = k.model_name().to_string();
                    st.keyboard.firmware = info.map(|i| i.firmware_string()).unwrap_or_default();
                    Ok("Clavier détecté".into())
                }
                Err(e) => Err(e.to_string()),
            },
            (Some(k), KeyboardCommand::Apply(look)) => {
                let r = match &look {
                    KeyboardLook::Color { color, brightness } => k
                        .set_color(scale(*color, *brightness), 100)
                        .map(|_| format!("Couleur {} appliquée", color.hex())),
                    KeyboardLook::Effect(s) => k
                        .apply_effect(s)
                        .map(|_| format!("Effet « {} » appliqué", s.effect.label())),
                    KeyboardLook::Off => k.turn_off().map(|_| "Rétroéclairage éteint".to_string()),
                };
                r.map_err(|e| e.to_string())
            }
            (Some(k), KeyboardCommand::RestoreDefault) => k
                .restore_default()
                .map(|_| "Éclairage d'usine restauré".to_string())
                .map_err(|e| e.to_string()),
        };
        if result.is_err() {
            // Device may have been re-enumerated (sleep/resume): reopen next time.
            kb = None;
        }
        let mut st = lock(&state);
        st.keyboard.busy = false;
        st.keyboard.available = Some(st.keyboard.available.unwrap_or(false) || result.is_ok());
        match result {
            Ok(m) => {
                st.keyboard.message = m;
                st.keyboard.error = false;
            }
            Err(m) => {
                log_line(&format!("keyboard: {m}"));
                if st.keyboard.available != Some(true) {
                    st.keyboard.available = Some(false);
                }
                st.keyboard.message = m;
                st.keyboard.error = true;
            }
        }
    }
}

/// Default EffectSettings re-export for the interface.
pub fn default_effect() -> EffectSettings {
    EffectSettings::default()
}

// ---------------------------------------------------------------------------
// Control worker
// ---------------------------------------------------------------------------

struct Worker {
    state: Arc<Mutex<Snapshot>>,
    cfg: Config,
    start: Instant,
    auto: AutoEngine,
    applied: Option<Level>,
    /// Software fan control currently active (custom curve or manual level).
    sw_fans: bool,
    manual_fan: Option<u8>,
    policy: u8,
    pdh: Option<PdhSensors>,
    nvml: Option<Nvml>,
    nvml_tried: bool,
    last_game_scan: Option<Instant>,
    game: Option<String>,
    /// Last time max fan was (re)sent; the BIOS may drop it on its own.
    last_max_fan_kick: Instant,
}

fn ok_code<T>(r: windows::core::Result<(i32, T)>) -> Option<T> {
    match r {
        Ok((0, v)) => Some(v),
        _ => None,
    }
}

fn describe(r: windows::core::Result<i32>) -> Result<(), String> {
    match r {
        Ok(0) => Ok(()),
        Ok(3) => Err("non supporté par ce BIOS (code 3)".into()),
        Ok(c) => Err(format!("refusé par le BIOS (code {c})")),
        Err(e) if e.code().0 as u32 == 0x8004_1003 => {
            Err("accès refusé : lancez l'application en administrateur".into())
        }
        Err(e) => Err(e.message().to_string()),
    }
}

impl Worker {
    fn new(state: Arc<Mutex<Snapshot>>, cfg: Config) -> Self {
        Self {
            state,
            cfg,
            start: Instant::now(),
            auto: AutoEngine::new(),
            applied: None,
            sw_fans: false,
            manual_fan: None,
            policy: 1,
            pdh: None,
            nvml: None,
            nvml_tried: false,
            last_game_scan: None,
            game: None,
            last_max_fan_kick: Instant::now(),
        }
    }

    fn message(&self, text: impl Into<String>, error: bool) {
        let text = text.into();
        log_line(&format!("action: {text}"));
        let mut st = lock(&self.state);
        st.message = text;
        st.message_is_error = error;
    }

    fn run(mut self, rx: Receiver<Command>) {
        log_line("control: starting");
        self.pdh = PdhSensors::new();

        // The connection itself succeeds without admin rights; only the
        // method calls are refused. Probe one read to know for sure.
        let client = match BiosClient::connect().and_then(|c| c.get_system().map(|_| c)) {
            Ok(c) => Some(c),
            Err(e) => {
                let msg = if is_elevated() {
                    format!("Interface BIOS HP indisponible ({})", e.message())
                } else {
                    "Droits administrateur requis pour piloter les ventilateurs et les modes.".into()
                };
                log_line(&format!("control: connect failed: {e}"));
                lock(&self.state).connect_error = Some(msg);
                None
            }
        };

        if let Some(c) = &client {
            self.read_static(c);
            // Re-apply the saved mode at startup.
            let mode = self.cfg.mode;
            self.set_mode(c, mode, false);
        }

        let mut last_tick = Instant::now();
        let mut last_slow = Instant::now() - Duration::from_secs(10);
        let mut last_reassert = Instant::now();
        loop {
            match rx.recv_timeout(Duration::from_millis(250)) {
                Ok(Command::Shutdown) => {
                    if let Some(c) = &client {
                        self.release_fans(c);
                    }
                    log_line("control: shutdown");
                    return;
                }
                Ok(cmd) => self.handle(client.as_ref(), cmd),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }

            if last_tick.elapsed() < Duration::from_secs(1) {
                continue;
            }
            // A long gap means the PC slept: re-assert the hardware state.
            let resumed = last_tick.elapsed() > Duration::from_secs(15);
            last_tick = Instant::now();

            self.sample_fast(client.as_ref());
            if last_slow.elapsed() >= Duration::from_secs(2) {
                last_slow = Instant::now();
                if let Some(c) = &client {
                    self.sample_bios(c);
                }
            }
            if let Some(c) = &client {
                if resumed || last_reassert.elapsed() > Duration::from_secs(300) {
                    last_reassert = Instant::now();
                    if let Some(level) = self.applied.take() {
                        log_line("control: re-asserting state");
                        self.apply_level(c, level, true);
                    }
                }
                self.tick_modes(c);
            }
            self.push_history();
        }
    }

    fn read_static(&mut self, c: &BiosClient) {
        let system = ok_code(c.get_system());
        let fans = ok_code(c.get_fan_type_info());
        let gpu_power = ok_code(c.get_gpu_power());
        let gpu_mode = ok_code(c.get_gpu_mode());
        let fan_table = ok_code(c.get_fan_table());
        let adapter = ok_code(c.get_adapter_status());
        if let Some(s) = &system {
            self.policy = s.thermal_policy;
        }
        let mut st = lock(&self.state);
        st.connected = true;
        st.system = system;
        st.fan_count = fans.map(|f| f.fan_count()).unwrap_or(2).max(1);
        st.gpu_power = gpu_power;
        st.gpu_mode = gpu_mode;
        st.fan_table = fan_table;
        st.adapter = adapter;
    }

    fn sample_fast(&mut self, _c: Option<&BiosClient>) {
        let power = sensors::power_state();
        let pdh = self.pdh.as_mut().map(|p| p.sample()).unwrap_or_default();

        let gpu_allowed = power.on_ac || self.cfg.gpu_sensors_on_battery;
        if gpu_allowed && self.nvml.is_none() && !self.nvml_tried {
            self.nvml_tried = true;
            self.nvml = Nvml::load();
            log_line(&format!("control: nvml {}", if self.nvml.is_some() { "ok" } else { "unavailable" }));
        }
        let gpu = if gpu_allowed {
            self.nvml.as_ref().map(|n| n.sample()).unwrap_or_default()
        } else {
            GpuSample::default()
        };

        let scan_due = self
            .last_game_scan
            .map(|t| t.elapsed() >= Duration::from_secs(3))
            .unwrap_or(true);
        let mut vendor = None;
        if scan_due {
            self.last_game_scan = Some(Instant::now());
            let procs = games::processes();
            self.game = games::detect_in(&procs, &self.cfg.auto.extra_games, &self.cfg.auto.ignored_games);
            vendor = Some(procs.iter().any(|p| {
                p.name.starts_with("omencommandcenter") || p.name.starts_with("hp.omen.omencommandcenter")
            }));
        }

        let mut st = lock(&self.state);
        st.power = power;
        st.cpu_load = pdh.cpu_load;
        st.gpu_load = pdh.gpu_load.or(gpu.load);
        st.thermal_zone = pdh.thermal_zone;
        st.gpu = gpu;
        st.gpu_sensors_paused = !gpu_allowed && self.nvml.is_some();
        st.gpu_name = self.nvml.as_ref().map(|n| n.name.clone());
        st.game = self.game.clone();
        if let Some(v) = vendor {
            st.vendor_app_running = v;
        }
    }

    fn sample_bios(&mut self, c: &BiosClient) {
        let fans = ok_code(c.get_fan_level());
        let max_fan = ok_code(c.get_max_fan());
        let ir = ok_code(c.get_sensor(Sensor::Ir));
        let amb = ok_code(c.get_sensor(Sensor::Ambient));
        let pch = ok_code(c.get_sensor(Sensor::Pch));
        let vr = ok_code(c.get_sensor(Sensor::Vr));
        let thr = ok_code(c.get_throttling());
        let mut st = lock(&self.state);
        if let Some(f) = fans {
            st.fan_cpu = Some(f.cpu);
            st.fan_gpu = Some(f.gpu);
        }
        st.max_fan = max_fan.or(st.max_fan);
        drop(st);
        self.keep_max_fan(c, max_fan);
        let mut st = lock(&self.state);
        let valid = |t: Option<u8>| t.filter(|v| *v > 0 && *v < 120);
        st.temp_ir = valid(ir);
        st.temp_ambient = valid(amb);
        st.temp_pch = valid(pch);
        st.temp_vr = valid(vr);
        st.throttling = thr == Some(Throttling::On);
    }

    /// The BIOS can leave max fan on its own after a while (seen live: back to
    /// ~2000 rpm within 3 min). While a level wants max fan, re-send it as soon as
    /// the readback says off, and every 60 s anyway.
    fn keep_max_fan(&mut self, c: &BiosClient, readback: Option<bool>) {
        if !self.applied.is_some_and(|l| l.max_fan()) {
            return;
        }
        let dropped = readback == Some(false);
        if dropped || self.last_max_fan_kick.elapsed() >= Duration::from_secs(60) {
            if dropped {
                log_line("control: BIOS dropped max fan, re-applying");
            }
            self.last_max_fan_kick = Instant::now();
            if c.set_max_fan(true).is_ok_and(|code| code == 0) {
                lock(&self.state).max_fan = Some(true);
            }
        }
    }

    fn push_history(&self) {
        let mut st = lock(&self.state);
        let p = HistoryPoint {
            t: self.start.elapsed().as_secs_f64(),
            cpu_load: st.cpu_load.unwrap_or(0.0),
            gpu_temp: st.gpu.temp,
            fan_cpu_rpm: Snapshot::fan_rpm(st.fan_cpu),
            fan_gpu_rpm: Snapshot::fan_rpm(st.fan_gpu),
        };
        st.history.push_back(p);
        while st.history.len() > 600 {
            st.history.pop_front();
        }
    }

    fn inputs(&self) -> Inputs {
        let st = lock(&self.state);
        Inputs {
            cpu_load: st.cpu_load.unwrap_or(0.0),
            gpu_load: st.gpu_load,
            gpu_temp: st.gpu.temp,
            cpu_temp: st.thermal_zone,
            throttling: st.throttling,
            on_ac: st.power.on_ac,
            game: st.game.clone(),
        }
    }

    fn tick_modes(&mut self, c: &BiosClient) {
        let now = self.start.elapsed().as_secs_f64();
        match self.cfg.mode {
            Mode::Auto => {
                let d = self.auto.update(now, &self.inputs(), &self.cfg.auto);
                if self.applied != Some(d.level) {
                    log_line(&format!("auto: {:?} ({})", d.level, d.reason));
                    self.apply_level(c, d.level, false);
                }
                lock(&self.state).auto_reason = d.reason;
            }
            Mode::Custom => {
                let temp = {
                    let st = lock(&self.state);
                    [st.gpu.temp, st.cpu_temp(), st.temp_ir.map(f32::from)]
                        .into_iter()
                        .flatten()
                        .fold(0.0f32, f32::max)
                };
                if temp > 0.0 {
                    let (cpu, gpu) = self.cfg.custom_curve.interpolate(temp.round() as u8);
                    if describe(c.set_fan_levels(cpu, gpu, 0xFF)).is_ok() {
                        lock(&self.state).custom_levels = Some((cpu, gpu));
                    }
                }
            }
            _ => {}
        }
        if let Some(level) = self.manual_fan {
            // Keep the manual level asserted (BIOS may override after events).
            let _ = c.set_fan_levels(level, level, 0xFF);
        }
    }

    fn release_fans(&mut self, c: &BiosClient) {
        if self.sw_fans || self.manual_fan.is_some() {
            let level = self.applied.unwrap_or(Level::Balanced);
            let _ = c.set_performance_profile(level.profile(), self.policy, true);
            self.sw_fans = false;
        }
    }

    /// Writes a level to the hardware. Returns an error text when a step failed.
    fn apply_level(&mut self, c: &BiosClient, level: Level, quiet: bool) {
        let sw = self.cfg.mode == Mode::Custom || self.manual_fan.is_some();
        let mut errors = Vec::new();

        // Thermal mode first, max fan last (same order as the Hub): writing 0x1A
        // after 0x27 lets the BIOS drop max fan again.
        if let Err(e) = describe(c.set_performance_profile(level.profile(), self.policy, !sw)) {
            errors.push(format!("mode thermique : {e}"));
        }
        if let Err(e) = describe(c.set_max_fan(level.max_fan())) {
            errors.push(format!("ventilateurs max : {e}"));
        }
        self.last_max_fan_kick = Instant::now();
        let (tgp, ppab) = level.gpu_power();
        let current = ok_code(c.get_gpu_power());
        let wanted = GpuPowerData {
            custom_tgp: u8::from(tgp),
            ppab: u8::from(ppab),
            d_state: current.map(|g| g.d_state).filter(|d| *d != 0).unwrap_or(1),
            peak_temperature: current.map(|g| g.peak_temperature).filter(|t| *t != 0).unwrap_or(87),
        };
        if current.map(|g| (g.custom_tgp, g.ppab)) != Some((wanted.custom_tgp, wanted.ppab)) {
            if let Err(e) = describe(c.set_gpu_power(wanted)) {
                errors.push(format!("puissance GPU : {e}"));
            }
        }
        let readback = ok_code(c.get_gpu_power());
        self.sw_fans = sw;
        self.applied = Some(level);
        {
            let mut st = lock(&self.state);
            st.applied = Some(level);
            if readback.is_some() {
                st.gpu_power = readback;
            }
            st.max_fan = ok_code(c.get_max_fan()).or(Some(level.max_fan()));
        }
        if !errors.is_empty() {
            self.message(errors.join(" ; "), true);
        } else if !quiet && self.cfg.mode != Mode::Auto {
            self.message(format!("Mode {} appliqué", level.label()), false);
        }
    }

    fn set_mode(&mut self, c: &BiosClient, mode: Mode, announce: bool) {
        let was_sw = self.sw_fans;
        self.cfg.mode = mode;
        lock(&self.state).mode = mode;
        if mode != Mode::Custom {
            lock(&self.state).custom_levels = None;
        }
        match mode.fixed_level() {
            Some(level) => {
                self.applied = None;
                self.apply_level(c, level, !announce);
            }
            None => {
                self.auto = AutoEngine::new();
                self.applied = None;
                let d = self.auto.update(self.start.elapsed().as_secs_f64(), &self.inputs(), &self.cfg.auto);
                self.apply_level(c, d.level, true);
                lock(&self.state).auto_reason = d.reason;
                if announce {
                    self.message("Mode Auto activé", false);
                }
            }
        }
        if was_sw && !self.sw_fans {
            log_line("control: fans returned to BIOS control");
        }
    }

    fn handle(&mut self, client: Option<&BiosClient>, cmd: Command) {
        let Some(c) = client else {
            match cmd {
                Command::SetAutoSettings(s) => self.cfg.auto = s,
                Command::SetMode(m) => {
                    self.cfg.mode = m;
                    lock(&self.state).mode = m;
                    self.message("Impossible : interface BIOS non connectée (droits administrateur ?)", true);
                }
                _ => self.message("Impossible : interface BIOS non connectée (droits administrateur ?)", true),
            }
            return;
        };
        match cmd {
            Command::SetMode(m) => self.set_mode(c, m, true),
            Command::SetAutoSettings(s) => self.cfg.auto = s,
            Command::SetCustomCurve(curve) => {
                self.cfg.custom_curve = curve;
                if self.cfg.mode == Mode::Custom {
                    self.message("Courbe appliquée", false);
                }
            }
            Command::SetManualFan(level) => {
                self.manual_fan = level;
                lock(&self.state).manual_fan = level;
                match level {
                    Some(l) => {
                        // Take fan regulation away from the BIOS first (0x1A,
                        // software control), then write the levels.
                        let current = self.applied.unwrap_or(Level::Balanced);
                        self.applied = None;
                        self.apply_level(c, current, true);
                        let result = describe(c.set_fan_levels(l, l, 0xFF));
                        match result {
                        Ok(()) => self.message(format!("Ventilateurs fixés à ~{} tr/min", omen_wmi::level_to_rpm(l)), false),
                        Err(e) => self.message(format!("Ventilateurs : {e}"), true),
                        }
                    }
                    None => {
                        let level = self.applied.unwrap_or(Level::Balanced);
                        self.applied = None;
                        self.apply_level(c, level, true);
                        self.message("Ventilateurs rendus au pilotage automatique", false);
                    }
                }
            }
            Command::SetGpuSensorsOnBattery(v) => {
                self.cfg.gpu_sensors_on_battery = v;
            }
            Command::SetGpuMux(mode) => match describe(c.set_gpu_mode(mode)) {
                Ok(()) => {
                    lock(&self.state).gpu_mode = ok_code(c.get_gpu_mode()).or(Some(mode));
                    self.message("Mode graphique enregistré : redémarrez le PC pour l'appliquer", false)
                }
                Err(e) => self.message(format!("Mode graphique : {e}"), true),
            },
            Command::SetGpuPower { custom_tgp, ppab } => {
                let current = ok_code(c.get_gpu_power()).unwrap_or(GpuPowerData {
                    custom_tgp: 0,
                    ppab: 0,
                    d_state: 1,
                    peak_temperature: 87,
                });
                let data = GpuPowerData {
                    custom_tgp: u8::from(custom_tgp),
                    ppab: u8::from(ppab),
                    ..current
                };
                match describe(c.set_gpu_power(data)) {
                    Ok(()) => {
                        lock(&self.state).gpu_power = ok_code(c.get_gpu_power());
                        self.message("Puissance GPU appliquée", false);
                    }
                    Err(e) => self.message(format!("Puissance GPU : {e}"), true),
                }
            }
            Command::RefreshInfo => self.read_static(c),
            Command::Keyboard(_) | Command::Shutdown => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brightness_scales_colors() {
        assert_eq!(scale(Rgb::new(255, 100, 0), 100), Rgb::new(255, 100, 0));
        assert_eq!(scale(Rgb::new(255, 100, 0), 50), Rgb::new(128, 50, 0));
        assert_eq!(scale(Rgb::new(255, 255, 255), 0), Rgb::new(0, 0, 0));
    }
}
