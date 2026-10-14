//! OMEN Hub RS — GUI dashboard, fan curve editor, monitoring and tray.
//!
//! Architecture: a worker thread owns the `BiosClient` (COM lives there) and
//! talks to the GUI through a command channel and a shared state mutex.

use std::collections::VecDeque;
use std::io::Write as _;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use eframe::egui;
use egui_plot::{Legend, Line, Plot, PlotPoints};
use omen_wmi::{
    Backlight, BiosClient, ColorTable, FanCurve, FanMode, FanTable, GpuMode, GpuPowerData,
    KbdType, SensorSnapshot,
};

pub const APP_TITLE: &str = "OMEN Hub RS";

fn log_path() -> Option<PathBuf> {
    let base = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(base).join("omen-rs").join("omen-hub.log"))
}

pub fn log_line(msg: &str) {
    if let Some(path) = log_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let _ = writeln!(f, "[{ts}] {msg}");
        }
    }
}

/// Locks the shared state, recovering from a poisoned mutex instead of
/// panicking (a worker panic must not take the whole GUI down).
fn lock<'a>(state: &'a Arc<Mutex<UiState>>) -> MutexGuard<'a, UiState> {
    state.lock().unwrap_or_else(|e| e.into_inner())
}

/// Extracts the value of a BIOS read, recording the first failure so the UI
/// can surface it instead of silently showing nothing.
fn take<T>(
    r: windows::core::Result<(i32, T)>,
    what: &str,
    err: &mut Option<String>,
) -> Option<T> {
    match r {
        Ok((0, v)) => Some(v),
        Ok((c, _)) => {
            if err.is_none() {
                *err = Some(format!("{what}: code BIOS {c}"));
            }
            None
        }
        Err(e) => {
            if err.is_none() {
                *err = Some(format!("{what}: {e}"));
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Commands / shared state
// ---------------------------------------------------------------------------

pub enum Cmd {
    SetMaxFan(bool),
    SetFanMode(FanMode),
    SetLevels(u8, u8),
    /// `None` disables the software curve.
    SetCurve(Option<FanCurve>),
    SetGpuPower(GpuPowerData),
    SetBacklight(Backlight),
    SetColorTable(ColorTable),
    ReadFanTable,
    ReadKeyboard,
    ReadGpuMode,
    SampleNow,
}

#[derive(Default)]
pub struct UiState {
    pub snapshot: SensorSnapshot,
    /// (seconds since start, temp C, cpu %, gpu %)
    pub history: VecDeque<(f64, f32, f32, f32)>,
    pub fan_table: Option<FanTable>,
    pub kbd_type: Option<KbdType>,
    pub backlight: Option<Backlight>,
    pub color_table: Option<ColorTable>,
    pub gpu_mode: Option<GpuMode>,
    pub curve: FanCurve,
    pub curve_enabled: bool,
    pub curve_levels: Option<(u8, u8)>,
    pub status: String,
    pub connect_error: Option<String>,
    pub elevated: bool,
    pub connected: bool,
    pub last_sample_at: Option<Instant>,
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var("APPDATA").ok()?;
    Some(PathBuf::from(base).join("omen-rs").join("config.json"))
}

#[derive(serde::Serialize, serde::Deserialize, Default)]
struct Config {
    curve: Option<FanCurve>,
    curve_enabled: bool,
    fan_mode: Option<u8>,
}

fn load_config() -> Config {
    config_path()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_config(cfg: &Config) {
    if let Some(path) = config_path() {
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(cfg) {
            let _ = std::fs::write(path, json);
        }
    }
}

// ---------------------------------------------------------------------------
// Worker thread
// ---------------------------------------------------------------------------

fn spawn_worker(rx_cmd: Receiver<Cmd>, state: Arc<Mutex<UiState>>) {
    std::thread::spawn(move || {
        log_line("worker: starting");
        let start = Instant::now();

        let client = match BiosClient::connect() {
            Ok(c) => {
                log_line("worker: connected");
                c
            }
            Err(e) => {
                log_line(&format!("worker: connect failed: {e}"));
                lock(&state).connect_error =
                    Some(format!("Connexion WMI impossible ({e}). Lance l'app en administrateur."));
                return;
            }
        };

        // Initial reads: collect first, then store. The shared mutex is never
        // held across a WMI call, so a slow or stuck call can never freeze the UI.
        let system = client
            .get_system()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));
        let gpu = client
            .get_gpu_power()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));
        let gpu_mode = client
            .get_gpu_mode()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));
        let kbd = client
            .get_kbd_type()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));
        let backlight = client
            .get_backlight()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));
        let color = client
            .get_color_table()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));
        let table = client
            .get_fan_table()
            .ok()
            .and_then(|(c, v)| (c == 0).then_some(v));

        {
            let mut st = lock(&state);
            if let Some(v) = system {
                st.snapshot.system = Some(v);
            }
            if let Some(v) = gpu {
                st.snapshot.gpu_power = Some(v);
            }
            if let Some(v) = gpu_mode {
                st.gpu_mode = Some(v);
            }
            if let Some(v) = kbd {
                st.kbd_type = Some(v);
            }
            if let Some(v) = backlight {
                st.backlight = Some(v);
            }
            if let Some(v) = color {
                st.color_table = Some(v);
            }
            if let Some(v) = table {
                st.fan_table = Some(v);
            }
            st.connected = true;
            st.status = "Connecté".to_string();
        }
        log_line("worker: initial reads done");

        sample(&client, &state, start);
        log_line("worker: first sample done");

        let mut last_sample = Instant::now();
        let mut heartbeat = Instant::now();

        loop {
            match rx_cmd.recv_timeout(Duration::from_millis(400)) {
                Ok(cmd) => handle_cmd(&client, &state, cmd, start),
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => break,
            }

            if last_sample.elapsed() >= Duration::from_secs(2) {
                sample(&client, &state, start);
                apply_curve(&client, &state);
                last_sample = Instant::now();
            }

            if heartbeat.elapsed() >= Duration::from_secs(30) {
                let st = lock(&state);
                log_line(&format!(
                    "worker: heartbeat temp={:?} fan_cpu={:?} connected={} error={:?}",
                    st.snapshot.temp_c, st.snapshot.fan_cpu, st.connected, st.snapshot.error
                ));
                drop(st);
                heartbeat = Instant::now();
            }
        }
    });
}

fn set_status(state: &Arc<Mutex<UiState>>, msg: impl Into<String>) {
    lock(state).status = msg.into();
}

fn check(state: &Arc<Mutex<UiState>>, code: i32, what: &str) -> bool {
    if code == 0 {
        true
    } else {
        set_status(state, format!("{what}: code BIOS {code}"));
        false
    }
}

fn sample(client: &BiosClient, state: &Arc<Mutex<UiState>>, start: Instant) {
    // Collect without the lock, then store: never hold the mutex across WMI calls.
    let mut err: Option<String> = None;
    let temp = take(client.get_temperature(), "temperature", &mut err);
    let fan = take(client.get_fan_level(), "fan level", &mut err);
    let max_fan = take(client.get_max_fan(), "max fan", &mut err);
    let throttling = take(client.get_throttling(), "throttling", &mut err).map(|v| format!("{v:?}"));
    let power = take(client.get_power_data(), "power data", &mut err);

    let mut st = lock(state);
    if let Some(v) = temp {
        st.snapshot.temp_c = Some(v);
    }
    if let Some(v) = fan {
        st.snapshot.fan_cpu = Some(v.cpu);
        st.snapshot.fan_gpu = Some(v.gpu);
        st.snapshot.fan_cpu_percent = Some(v.cpu_percent());
        st.snapshot.fan_gpu_percent = Some(v.gpu_percent());
    }
    if let Some(v) = max_fan {
        st.snapshot.max_fan = Some(v);
    }
    if let Some(v) = throttling {
        st.snapshot.throttling = Some(v);
    }
    if let Some(v) = power {
        st.snapshot.power_raw = v;
    }
    st.snapshot.error = err;
    st.last_sample_at = Some(Instant::now());

    let t = start.elapsed().as_secs_f64();
    let temp = st.snapshot.temp_c.unwrap_or(0) as f32;
    let cpu = st.snapshot.fan_cpu_percent.unwrap_or(0) as f32;
    let gpu = st.snapshot.fan_gpu_percent.unwrap_or(0) as f32;
    st.history.push_back((t, temp, cpu, gpu));
    while st.history.len() > 400 {
        st.history.pop_front();
    }
}

fn apply_curve(client: &BiosClient, state: &Arc<Mutex<UiState>>) {
    let (enabled, curve, temp, max_fan) = {
        let st = lock(state);
        (
            st.curve_enabled,
            st.curve.clone(),
            st.snapshot.temp_c,
            st.snapshot.max_fan.unwrap_or(false),
        )
    };
    if !enabled || max_fan {
        return;
    }
    let Some(temp) = temp else { return };
    let (cpu, gpu) = curve.interpolate(temp);
    if let Ok(code) = client.set_fan_level(cpu, gpu) {
        if code == 0 {
            let mut st = lock(state);
            st.curve_levels = Some((cpu, gpu));
        }
    }
}

fn handle_cmd(client: &BiosClient, state: &Arc<Mutex<UiState>>, cmd: Cmd, start: Instant) {
    match cmd {
        Cmd::SetMaxFan(on) => {
            match client.set_max_fan(on) {
                Ok(code) if check(state, code, "max fan") => {
                    set_status(state, format!("Max fan: {}", if on { "ON" } else { "off" }));
                    let mut st = lock(state);
                    st.snapshot.max_fan = Some(on);
                }
                Ok(_) => {}
                Err(e) => set_status(state, format!("max fan: {e}")),
            };
        }
        Cmd::SetFanMode(mode) => match client.set_fan_mode(mode) {
            Ok(code) if check(state, code, "fan mode") => {
                set_status(state, format!("Mode: {mode}"));
                let mut cfg = load_config();
                cfg.fan_mode = Some(mode as u8);
                save_config(&cfg);
            }
            Ok(_) => {}
            Err(e) => set_status(state, format!("fan mode: {e}")),
        },
        Cmd::SetLevels(cpu, gpu) => match client.set_fan_level(cpu, gpu) {
            Ok(code) if check(state, code, "fan levels") => {
                set_status(state, format!("Niveaux manuels: CPU=0x{cpu:02X} GPU=0x{gpu:02X}"));
            }
            Ok(_) => {}
            Err(e) => set_status(state, format!("fan levels: {e}")),
        },
        Cmd::SetCurve(curve) => {
            let mut st = lock(state);
            st.curve_enabled = curve.is_some();
            if let Some(c) = curve {
                st.curve = c;
            }
            let (curve, enabled) = (st.curve.clone(), st.curve_enabled);
            drop(st);
            let mut cfg = load_config();
            cfg.curve = Some(curve);
            cfg.curve_enabled = enabled;
            save_config(&cfg);
            set_status(
                state,
                if enabled {
                    "Courbe logicielle activée"
                } else {
                    "Courbe logicielle désactivée"
                },
            );
            if enabled {
                apply_curve(client, state);
            }
        }
        Cmd::SetGpuPower(data) => match client.set_gpu_power(data) {
            Ok(code) if check(state, code, "gpu power") => {
                set_status(state, format!("GPU power: {data:?}"));
                if let Ok((_, gpu)) = client.get_gpu_power() {
                    lock(state).snapshot.gpu_power = Some(gpu);
                }
            }
            Ok(_) => {}
            Err(e) => set_status(state, format!("gpu power: {e}")),
        },
        Cmd::SetBacklight(value) => match client.set_backlight(value) {
            Ok(code) if check(state, code, "backlight") => {
                set_status(state, format!("Rétroéclairage: {value:?}"));
                if let Ok((_, bl)) = client.get_backlight() {
                    lock(state).backlight = Some(bl);
                }
            }
            Ok(_) => {}
            Err(e) => set_status(state, format!("backlight: {e}")),
        },
        Cmd::SetColorTable(table) => match client.set_color_table(&table) {
            Ok(code) if check(state, code, "color table") => {
                set_status(state, "Couleurs appliquées");
                if let Ok((_, ct)) = client.get_color_table() {
                    lock(state).color_table = Some(ct);
                }
            }
            Ok(_) => {}
            Err(e) => set_status(state, format!("color table: {e}")),
        },
        Cmd::ReadFanTable => match client.get_fan_table() {
            Ok((code, ft)) if check(state, code, "fan table") => {
                lock(state).fan_table = Some(ft);
                set_status(state, "Table BIOS relue");
            }
            Ok(_) => {}
            Err(e) => set_status(state, format!("fan table: {e}")),
        },
        Cmd::ReadKeyboard => {
            let mut st = lock(state);
            if let Ok((_, kbd)) = client.get_kbd_type() {
                st.kbd_type = Some(kbd);
            }
            if let Ok((_, bl)) = client.get_backlight() {
                st.backlight = Some(bl);
            }
            if let Ok((_, ct)) = client.get_color_table() {
                st.color_table = Some(ct);
            }
        }
        Cmd::ReadGpuMode => {
            if let Ok((_, mode)) = client.get_gpu_mode() {
                lock(state).gpu_mode = Some(mode);
            }
        }
        Cmd::SampleNow => sample(client, state, start),
    }
}

// ---------------------------------------------------------------------------
// Tray
// ---------------------------------------------------------------------------

struct Tray {
    _icon: tray_icon::TrayIcon,
    show: tray_icon::menu::MenuId,
    max_on: tray_icon::menu::MenuId,
    max_off: tray_icon::menu::MenuId,
    mode_performance: tray_icon::menu::MenuId,
    mode_default: tray_icon::menu::MenuId,
    mode_cool: tray_icon::menu::MenuId,
    curve_toggle: tray_icon::menu::MenuId,
    quit: tray_icon::menu::MenuId,
}

fn make_icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c + 0.5;
            let dy = y as f32 - c + 0.5;
            let d = (dx * dx + dy * dy).sqrt();
            let idx = ((y * size + x) * 4) as usize;
            if d <= c - 1.0 {
                let t = (d / c).clamp(0.0, 1.0);
                rgba[idx] = (240.0 * (1.0 - t * 0.25)) as u8;
                rgba[idx + 1] = (110.0 * (1.0 - t * 0.7)) as u8;
                rgba[idx + 2] = (60.0 * (1.0 - t * 0.7)) as u8;
                rgba[idx + 3] = 255;
            } else {
                rgba[idx + 3] = 0;
            }
        }
    }
    rgba
}

fn build_tray() -> Option<Tray> {
    use tray_icon::menu::{Menu, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    let show = MenuItem::new("Afficher", true, None);
    let max_on = MenuItem::new("Max fan ON", true, None);
    let max_off = MenuItem::new("Max fan OFF", true, None);
    let mode_performance = MenuItem::new("Mode Performance", true, None);
    let mode_default = MenuItem::new("Mode Default", true, None);
    let mode_cool = MenuItem::new("Mode Cool", true, None);
    let curve_toggle = MenuItem::new("Activer/désactiver la courbe", true, None);
    let quit = MenuItem::new("Quitter", true, None);
    let sep = PredefinedMenuItem::separator();
    let _ = menu.append_items(&[
        &show,
        &sep,
        &max_on,
        &max_off,
        &PredefinedMenuItem::separator(),
        &mode_performance,
        &mode_default,
        &mode_cool,
        &PredefinedMenuItem::separator(),
        &curve_toggle,
        &PredefinedMenuItem::separator(),
        &quit,
    ]);

    let size = 32u32;
    let icon = tray_icon::Icon::from_rgba(make_icon_rgba(size), size, size).ok()?;

    let tray = tray_icon::TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip(APP_TITLE)
        .with_icon(icon)
        .build()
        .ok()?;

    Some(Tray {
        _icon: tray,
        show: show.id().clone(),
        max_on: max_on.id().clone(),
        max_off: max_off.id().clone(),
        mode_performance: mode_performance.id().clone(),
        mode_default: mode_default.id().clone(),
        mode_cool: mode_cool.id().clone(),
        curve_toggle: curve_toggle.id().clone(),
        quit: quit.id().clone(),
    })
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

struct App {
    state: Arc<Mutex<UiState>>,
    tx: Sender<Cmd>,
    tray: Option<Tray>,
    curve_edit: FanCurve,
    last_ui_log: Option<Instant>,
}

impl App {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = channel::<Cmd>();
        let state = Arc::new(Mutex::new(UiState::default()));

        let cfg = load_config();
        {
            let mut st = lock(&state);
            st.curve = cfg.curve.unwrap_or_else(FanCurve::default_curve);
            st.curve_enabled = cfg.curve_enabled;
            st.elevated = is_elevated();
        }
        log_line(&format!("app: elevated = {}", lock(&state).elevated));

        spawn_worker(rx, state.clone());

        let curve_edit = lock(&state).curve.clone();

        // Restore curve activation in the worker
        let cfg2 = load_config();
        if cfg2.curve_enabled {
            if let Some(curve) = cfg2.curve.clone() {
                let _ = tx.send(Cmd::SetCurve(Some(curve)));
            }
        }
        if let Some(mode) = cfg2.fan_mode.and_then(fan_mode_from_byte) {
            let _ = tx.send(Cmd::SetFanMode(mode));
        }

        Self {
            state,
            tx,
            tray: None,
            curve_edit,
            last_ui_log: None,
        }
    }

    fn poll_tray(&mut self, ctx: &egui::Context) {
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if let Some(tray) = &self.tray {
                if event.id == tray.quit {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                } else if event.id == tray.show {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id == tray.max_on {
                    let _ = self.tx.send(Cmd::SetMaxFan(true));
                } else if event.id == tray.max_off {
                    let _ = self.tx.send(Cmd::SetMaxFan(false));
                } else if event.id == tray.mode_performance {
                    let _ = self.tx.send(Cmd::SetFanMode(FanMode::Performance));
                } else if event.id == tray.mode_default {
                    let _ = self.tx.send(Cmd::SetFanMode(FanMode::Default));
                } else if event.id == tray.mode_cool {
                    let _ = self.tx.send(Cmd::SetFanMode(FanMode::Cool));
                } else if event.id == tray.curve_toggle {
                    let enabled = lock(&self.state).curve_enabled;
                    let curve = lock(&self.state).curve.clone();
                    if enabled {
                        let _ = self.tx.send(Cmd::SetCurve(None));
                    } else {
                        let _ = self.tx.send(Cmd::SetCurve(Some(curve)));
                    }
                }
            }
        }
        while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            if let tray_icon::TrayIconEvent::Click {
                button: tray_icon::MouseButton::Left,
                button_state: tray_icon::MouseButtonState::Up,
                ..
            } = event
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
    }

    fn dashboard(&mut self, ui: &mut egui::Ui) {
        let st = lock(&self.state);
        let snap = st.snapshot.clone();

        if let Some(e) = &snap.error {
            ui.colored_label(egui::Color32::RED, format!("⚠ {e}"));
        }

        ui.horizontal(|ui| {
            ui.heading(format!(
                "{} °C",
                snap.temp_c.map(|t| t.to_string()).unwrap_or_else(|| "--".into())
            ));
            ui.label(
                snap.throttling
                    .clone()
                    .map(|t| format!("throttling: {t}"))
                    .unwrap_or_default(),
            );
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label("Ventilateurs:");
            let cpu = snap.fan_cpu.map(|v| format!("0x{v:02X}")).unwrap_or("--".into());
            let gpu = snap.fan_gpu.map(|v| format!("0x{v:02X}")).unwrap_or("--".into());
            ui.label(format!("CPU {cpu} (~{} %)", snap.fan_cpu_percent.unwrap_or(0)));
            ui.label(format!("GPU {gpu} (~{} %)", snap.fan_gpu_percent.unwrap_or(0)));
        });
        if let Some((cpu, gpu)) = st.curve_levels {
            ui.label(format!("Courbe applique: CPU=0x{cpu:02X} GPU=0x{gpu:02X}"));
        }

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let max_on = snap.max_fan.unwrap_or(false);
            if ui
                .selectable_label(max_on, "Max fan ON")
                .clicked()
            {
                let _ = self.tx.send(Cmd::SetMaxFan(!max_on));
            }
            ui.separator();
            for (label, mode) in [
                ("Default", FanMode::Default),
                ("Performance", FanMode::Performance),
                ("Cool", FanMode::Cool),
                ("L0", FanMode::L0),
                ("L1", FanMode::L1),
                ("L3", FanMode::L3),
            ] {
                if ui.button(label).clicked() {
                    let _ = self.tx.send(Cmd::SetFanMode(mode));
                }
            }
        });

        ui.add_space(8.0);
        if let Some(gpu) = &snap.gpu_power {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "GPU: custom TGP {} / PPAB {} / seuil {} °C",
                    if gpu.custom_tgp == 1 { "on" } else { "off" },
                    if gpu.ppab == 1 { "on" } else { "off" },
                    gpu.peak_temperature
                ));
            });
        }
        ui.horizontal(|ui| {
            ui.label("GPU power:");
            for (label, data) in [
                (
                    "Min",
                    GpuPowerData {
                        custom_tgp: 0,
                        ppab: 0,
                        d_state: 1,
                        peak_temperature: 87,
                    },
                ),
                (
                    "Medium",
                    GpuPowerData {
                        custom_tgp: 1,
                        ppab: 0,
                        d_state: 1,
                        peak_temperature: 87,
                    },
                ),
                (
                    "Max",
                    GpuPowerData {
                        custom_tgp: 1,
                        ppab: 1,
                        d_state: 1,
                        peak_temperature: 87,
                    },
                ),
            ] {
                if ui.button(label).clicked() {
                    let _ = self.tx.send(Cmd::SetGpuPower(data));
                }
            }
        });

        if let Some(system) = &snap.system {
            ui.add_space(8.0);
            ui.label(format!(
                "PL4 défaut: {} W — limite concurrente GPU: {} W — GpuModeSwitch: 0x{:02X}",
                system.default_cpu_pl4, system.default_cpu_power_with_gpu, system.gpu_mode_switch
            ));
        }

        if !snap.power_raw.is_empty() {
            ui.add_space(4.0);
            let hex: Vec<String> = snap.power_raw.iter().take(16).map(|b| format!("{b:02X}")).collect();
            ui.label(format!("0x2A: {}", hex.join(" ")));
        }
        drop(st);
    }

    fn graph(&mut self, ui: &mut egui::Ui) {
        let history: Vec<(f64, f32, f32, f32)> =
            lock(&self.state).history.iter().copied().collect();
        let temp_pts: PlotPoints = history.iter().map(|(t, temp, _, _)| [*t, *temp as f64]).collect();
        let cpu_pts: PlotPoints = history.iter().map(|(t, _, cpu, _)| [*t, *cpu as f64]).collect();
        let gpu_pts: PlotPoints = history.iter().map(|(t, _, _, gpu)| [*t, *gpu as f64]).collect();

        Plot::new("history")
            .legend(Legend::default())
            .height(180.0)
            .allow_scroll(false)
            .show(ui, |plot_ui| {
                plot_ui.line(Line::new("Temp °C", temp_pts));
                plot_ui.line(Line::new("Fan CPU %", cpu_pts));
                plot_ui.line(Line::new("Fan GPU %", gpu_pts));
            });
    }

    fn curve_editor(&mut self, ui: &mut egui::Ui) {
        ui.label("Courbe logicielle (température → niveaux ventilo, appliquée toutes les 2 s via 0x2E).");
        ui.label("Ajuste les points, ajoute/supprime, puis Applique.");

        let mut remove: Option<usize> = None;
        for (i, p) in self.curve_edit.points.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("P{i}"));
                ui.label("T");
                ui.add(egui::DragValue::new(&mut p.temp_c).range(20..=100));
                ui.label("CPU");
                ui.add(egui::DragValue::new(&mut p.cpu_level).range(FanCurve::LEVEL_MIN..=FanCurve::LEVEL_MAX));
                ui.label("GPU");
                ui.add(egui::DragValue::new(&mut p.gpu_level).range(FanCurve::LEVEL_MIN..=FanCurve::LEVEL_MAX));
                if ui.small_button("✖").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            self.curve_edit.points.remove(i);
        }

        ui.horizontal(|ui| {
            if ui.button("Ajouter un point").clicked() {
                let last = self
                    .curve_edit
                    .points
                    .last()
                    .copied()
                    .unwrap_or(omen_wmi::FanCurvePoint {
                        temp_c: 40,
                        cpu_level: 0x1A,
                        gpu_level: 0x1A,
                    });
                self.curve_edit.points.push(omen_wmi::FanCurvePoint {
                    temp_c: (last.temp_c + 8).min(100),
                    cpu_level: last.cpu_level.saturating_add(4).min(FanCurve::LEVEL_MAX),
                    gpu_level: last.gpu_level.saturating_add(4).min(FanCurve::LEVEL_MAX),
                });
            }
            if ui.button("Trier par température").clicked() {
                self.curve_edit.points.sort_by_key(|p| p.temp_c);
            }
            if ui.button("Courbe par défaut").clicked() {
                self.curve_edit = FanCurve::default_curve();
            }
            ui.separator();
            if ui.button("Appliquer").clicked() {
                self.curve_edit.points.sort_by_key(|p| p.temp_c);
                let _ = self.tx.send(Cmd::SetCurve(Some(self.curve_edit.clone())));
            }
            let enabled = lock(&self.state).curve_enabled;
            if ui
                .selectable_label(enabled, if enabled { "Désactiver" } else { "Activer" })
                .clicked()
            {
                if enabled {
                    let _ = self.tx.send(Cmd::SetCurve(None));
                } else {
                    let _ = self.tx.send(Cmd::SetCurve(Some(self.curve_edit.clone())));
                }
            }
        });

        // Curve preview plot
        let pts_cpu: PlotPoints = self
            .curve_edit
            .points
            .iter()
            .map(|p| [p.temp_c as f64, p.cpu_level as f64])
            .collect();
        let pts_gpu: PlotPoints = self
            .curve_edit
            .points
            .iter()
            .map(|p| [p.temp_c as f64, p.gpu_level as f64])
            .collect();
        Plot::new("curve")
            .legend(Legend::default())
            .height(160.0)
            .allow_scroll(false)
            .show(ui, |plot_ui| {
                plot_ui.line(Line::new("CPU", pts_cpu));
                plot_ui.line(Line::new("GPU", pts_gpu));
            });

        if let Some(ft) = lock(&self.state).fan_table.clone() {
            ui.collapsing(format!("Table BIOS 0x2F ({} points, lecture seule)", ft.level_count), |ui| {
                for (i, e) in ft.entries.iter().enumerate() {
                    ui.label(format!(
                        "{i:02}: {} °C → CPU 0x{:02X} / GPU 0x{:02X}",
                        e.temp_c, e.cpu, e.gpu
                    ));
                }
            });
        }
        if ui.button("Relire la table BIOS").clicked() {
            let _ = self.tx.send(Cmd::ReadFanTable);
        }
    }

    fn keyboard(&mut self, ui: &mut egui::Ui) {
        let st = lock(&self.state);
        let kbd = st.kbd_type;
        let bl = st.backlight;
        let ct = st.color_table.clone();
        drop(st);

        ui.horizontal(|ui| {
            ui.label(format!("Type clavier: {kbd:?}"));
            ui.label(format!("Rétroéclairage: {bl:?}"));
            if ui.button("ON").clicked() {
                let _ = self.tx.send(Cmd::SetBacklight(Backlight::On));
            }
            if ui.button("OFF").clicked() {
                let _ = self.tx.send(Cmd::SetBacklight(Backlight::Off));
            }
            if ui.button("Relire").clicked() {
                let _ = self.tx.send(Cmd::ReadKeyboard);
            }
        });

        if let Some(ct) = ct {
            ui.add_space(4.0);
            let mut edited = ct.clone();
            let names = ["Zone droite", "Zone milieu", "Zone gauche", "Zones WASD"];
            for (i, z) in edited.zones.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(names.get(i).copied().unwrap_or("Zone"));
                    let mut color = [z.r, z.g, z.b];
                    if ui.color_edit_button_srgb(&mut color).changed() {
                        z.r = color[0];
                        z.g = color[1];
                        z.b = color[2];
                    }
                    ui.label(format!("#{:02X}{:02X}{:02X}", z.r, z.g, z.b));
                });
            }
            if ui.button("Appliquer les couleurs").clicked() {
                let _ = self.tx.send(Cmd::SetColorTable(edited));
            }
        } else {
            ui.label("Pas de table de couleurs lue (clavier sans RGB zones ?).");
        }

        ui.add_space(6.0);
        let mode = lock(&self.state).gpu_mode;
        ui.horizontal(|ui| {
            ui.label(format!("MUX GPU: {mode:?}"));
            if ui.button("Relire").clicked() {
                let _ = self.tx.send(Cmd::ReadGpuMode);
            }
        });
        ui.label("(changement MUX = redémarrage requis — disponible via CLI: omen gpu-mode …)");
    }
}

fn is_elevated() -> bool {
    #[cfg(windows)]
    unsafe {
        return windows::Win32::UI::Shell::IsUserAnAdmin().as_bool();
    }
    #[allow(unreachable_code)]
    false
}

fn fan_mode_from_byte(b: u8) -> Option<FanMode> {
    Some(match b {
        0x00 => FanMode::LegacyDefault,
        0x01 => FanMode::LegacyPerformance,
        0x02 => FanMode::LegacyCool,
        0x03 => FanMode::LegacyQuiet,
        0x04 => FanMode::LegacyExtreme,
        0x10 => FanMode::L0,
        0x11 => FanMode::L5,
        0x20 => FanMode::L1,
        0x21 => FanMode::L6,
        0x30 => FanMode::Default,
        0x31 => FanMode::Performance,
        0x40 => FanMode::L3,
        0x50 => FanMode::Cool,
        _ => return None,
    })
}

#[cfg(windows)]
fn relaunch_elevated() {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        if let Ok(exe) = std::env::current_exe() {
            let file = HSTRING::from(exe.to_string_lossy().to_string());
            ShellExecuteW(
                None,
                &HSTRING::from("runas"),
                &file,
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if self.tray.is_none() {
            self.tray = build_tray();
        }
        self.poll_tray(&ctx);
        ctx.request_repaint_after(Duration::from_millis(300));

        let (elevated, connect_error, status) = {
            let st = lock(&self.state);
            (st.elevated, st.connect_error.clone(), st.status.clone())
        };

        let (connected, last_sample_secs) = {
            let st = lock(&self.state);
            (st.connected, st.last_sample_at.map(|t| t.elapsed().as_secs()))
        };
        let stale = connected && last_sample_secs.map(|s| s > 8).unwrap_or(false);

        // Banners are rendered as plain widgets inside the central panel: the
        // top/bottom `Panel` API did not lay out reliably under `App::ui`.
        let mut banners: Vec<(egui::Color32, String)> = Vec::new();
        if !elevated {
            banners.push((
                egui::Color32::from_rgb(220, 140, 40),
                "⚠ Non élevé : les contrôles WMI nécessitent un jeton administrateur.".to_string(),
            ));
        }
        if let Some(err) = &connect_error {
            banners.push((egui::Color32::RED, err.clone()));
        }
        if stale {
            banners.push((
                egui::Color32::from_rgb(220, 140, 40),
                format!(
                    "⚠ Le worker ne répond plus depuis {} s (log: %APPDATA%\\omen-rs\\omen-hub.log)",
                    last_sample_secs.unwrap_or(0)
                ),
            ));
        } else if !connected && connect_error.is_none() {
            banners.push((egui::Color32::GRAY, "Connexion WMI en cours…".to_string()));
        }

        // UI beacon: proves the render thread is alive and what it sees.
        if self
            .last_ui_log
            .map(|t| t.elapsed().as_secs() >= 15)
            .unwrap_or(true)
        {
            let st = lock(&self.state);
            log_line(&format!(
                "ui: connected={} elevated={} temp={:?} fan=({:?},{:?}) system={} gpu_power={} banners={}",
                st.connected,
                st.elevated,
                st.snapshot.temp_c,
                st.snapshot.fan_cpu,
                st.snapshot.fan_gpu,
                st.snapshot.system.is_some(),
                st.snapshot.gpu_power.is_some(),
                banners.len()
            ));
            drop(st);
            self.last_ui_log = Some(Instant::now());
        }

        let figures = {
            let st = lock(&self.state);
            (
                st.snapshot.temp_c,
                st.snapshot.fan_cpu_percent,
                st.snapshot.fan_gpu_percent,
            )
        };

        egui::CentralPanel::default().show(ui, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (color, text) in &banners {
                    ui.colored_label(*color, text);
                }
                if !elevated && ui.button("Relancer en admin").clicked() {
                    #[cfg(windows)]
                    relaunch_elevated();
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
                ui.heading(APP_TITLE);
                ui.label(if status.is_empty() {
                    "Prêt".to_string()
                } else {
                    status.clone()
                });
                ui.label(format!(
                    "{} °C | fan {} % / {} %",
                    figures.0.map(|v| v.to_string()).unwrap_or("--".into()),
                    figures.1.unwrap_or(0),
                    figures.2.unwrap_or(0)
                ));
                ui.separator();
                self.dashboard(ui);
                ui.separator();
                ui.collapsing("Graphique (température & ventilateurs)", |ui| {
                    self.graph(ui);
                });
                ui.collapsing("Courbe ventilateurs", |ui| {
                    self.curve_editor(ui);
                });
                ui.collapsing("Clavier & MUX", |ui| {
                    self.keyboard(ui);
                });
            });
        });
    }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

pub fn run() -> Result<(), eframe::Error> {
    std::panic::set_hook(Box::new(|info| {
        log_line(&format!("PANIC: {info}"));
    }));
    log_line("omen-hub: starting");

    // Set the process COM security *before* winit/OLE can install its own:
    // the WMI provider requires at least impersonate level, and whatever runs
    // first wins for the whole process (later calls get RPC_E_TOO_LATE).
    unsafe {
        use windows::Win32::System::Com::{
            CoInitializeEx, CoInitializeSecurity, COINIT_APARTMENTTHREADED, EOAC_NONE,
            RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
        };
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let _ = CoInitializeSecurity(
            None,
            -1,
            None,
            None,
            RPC_C_AUTHN_LEVEL_DEFAULT,
            RPC_C_IMP_LEVEL_IMPERSONATE,
            None,
            EOAC_NONE,
            None,
        );
    }

    let size = 32u32;
    let icon = egui::IconData {
        rgba: make_icon_rgba(size),
        width: size,
        height: size,
    };

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([980.0, 760.0])
            .with_min_inner_size([760.0, 560.0])
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}
