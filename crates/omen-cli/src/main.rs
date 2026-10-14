use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use omen_core::{autostart, Level};
use omen_kbd::{Direction, Effect, EffectSettings, Keyboard, Rgb, Theme};
use omen_wmi::{
    Backlight, BiosClient, ColorTable, ColorZone, CpuPowerData, FanMode, GpuMode, GpuPowerData,
    Sensor, Throttling,
};

#[derive(Parser)]
#[command(
    name = "omen",
    about = "OMEN Hub RS en ligne de commande. Sans sous-commande : ouvre la fenêtre.",
    version
)]
struct Args {
    /// Start the window hidden in the notification area (used at logon)
    #[arg(long, global = true)]
    tray: bool,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Apply a mode once: silencieux | equilibre | performance | max (admin)
    Mode { mode: String },
    /// Live sensors: CPU/GPU load, temperatures, fans, power (BIOS part needs admin)
    #[command(alias = "capteurs")]
    Sensors,
    /// Keyboard: whole keyboard in one color (name or RRGGBB), e.g. `omen kbd-set rouge`
    #[command(alias = "kbd-color")]
    KbdSet {
        color: String,
        /// Intensity 5..100 %
        #[arg(long, default_value_t = 100)]
        intensite: u8,
    },
    /// Keyboard: animated effect (vague, respiration, cycle, onde, scintillement, trainee, pluie, confettis, omenx)
    KbdEffect {
        effect: String,
        /// 0 slow, 1 medium, 2 fast
        #[arg(long, default_value_t = 1)]
        vitesse: u8,
        /// Custom colors, e.g. --couleurs rouge,bleu (default: theme)
        #[arg(long)]
        couleurs: Option<String>,
        /// Theme when no colors: galaxie | volcan | jungle | ocean
        #[arg(long, default_value = "galaxie")]
        theme: String,
        /// gauche-droite | droite-gauche | haut | bas | centre | depuis-centre
        #[arg(long, default_value = "gauche-droite")]
        direction: String,
    },
    /// Keyboard: backlight off
    KbdOff,
    /// Keyboard: factory lighting
    KbdReset,
    /// Keyboard: MCU information (firmware, current effect)
    KbdInfo,
    /// Launch at Windows logon: on | off | status (on/off need admin)
    Autostart { state: String },
    /// Show system capabilities, fans, GPU power and thermal state
    Info,
    /// Diagnose the WMI connection step by step (run elevated)
    ConnectTest,
    /// Show the BIOS fan table (0x2F)
    FanTable,
    /// BIOS keyboard registers (note: they do not drive the per-key RGB LEDs)
    Kbd,
    /// BIOS backlight register: on | off | status
    Backlight { state: String },
    /// BIOS zone color register: "RRGGBB:RRGGBB:..." (1-4 zones), or read
    Color4 { colors: Option<String> },
    /// Graphics (MUX) mode: read, or set (reboot required, needs --yes)
    GpuMode {
        /// hybrid | discrete | optimus
        mode: Option<String>,
        #[arg(long)]
        yes: bool,
    },
    /// Monitoring loop (Ctrl+C to stop)
    Watch {
        #[arg(long, default_value_t = 2)]
        interval: u64,
    },
    /// Launch the GUI
    Gui,
    /// Raw BIOS command (RE experiments): omen raw <cmdType> <outSize> [hex bytes...]
    Raw {
        cmd_type: String,
        out_size: u32,
        data: Vec<String>,
    },
    /// Show current fan levels and max-fan state
    Fans,
    /// Read the BIOS thermal sensors (degrees Celsius)
    Temp,
    /// Read raw 0x2A power data
    Power,
    /// Show GPU power state (custom TGP / PPAB)
    #[command(alias = "gpupower")]
    GpuPower,
    /// Enable or disable maximum fan speed
    #[command(alias = "maxfan")]
    MaxFan { state: String },
    /// Set the raw thermal mode (0x1A)
    #[command(alias = "fanmode")]
    FanMode {
        /// default | performance | cool | l0 .. l6 | legacy-default | legacy-performance | legacy-cool | legacy-quiet | legacy-extreme
        mode: String,
    },
    /// Set fan levels (hundreds of RPM, observed range 0x14..0x3B). Stays until another mode is set.
    #[command(alias = "fanlevel")]
    FanLevel { cpu: u8, gpu: u8 },
    /// Set custom TGP / PPAB (values as read by `gpu-power`)
    GpuSet {
        custom_tgp: u8,
        ppab: u8,
        d_state: u8,
        peak_temperature: u8,
    },
    /// Set CPU power limits; use 255 to leave a limit unchanged
    CpuSet {
        pl1: u8,
        pl2: u8,
        pl4: u8,
        with_gpu: u8,
    },
}

fn parse_color4(spec: &str) -> Result<ColorTable> {
    let mut zones = Vec::new();
    for part in spec.split(':') {
        let c = parse_color(part)?;
        zones.push(ColorZone { r: c.r, g: c.g, b: c.b });
    }
    if zones.is_empty() || zones.len() > 4 {
        bail!("expected 1 to 4 zones, got {}", zones.len());
    }
    Ok(ColorTable {
        zone_count: (zones.len() - 1) as u8,
        zones,
    })
}

fn print_color_table(ct: &ColorTable) {
    for (i, z) in ct.zones.iter().enumerate() {
        println!("zone {i}: #{:02X}{:02X}{:02X}", z.r, z.g, z.b);
    }
}

fn check(code: i32, what: &str) -> Result<()> {
    match code {
        0 => Ok(()),
        3 => bail!("{what}: command not available on this platform (code 3)"),
        5 => bail!("{what}: insufficient buffer size (code 5)"),
        -1 => bail!("{what}: client-side exception (code -1)"),
        other => bail!("{what}: BIOS error code {other}"),
    }
}

fn admin_hint(e: &windows::core::Error) -> String {
    if e.code().0 as u32 == 0x8004_1003 || !omen_core::controller::is_elevated() {
        "accès refusé — ouvrez le terminal en administrateur (clic droit → Exécuter en tant qu'administrateur)".into()
    } else {
        e.message().to_string()
    }
}

fn parse_effect(name: &str) -> Result<Effect> {
    let n = name.to_lowercase().replace(['é', 'è'], "e").replace('î', "i");
    Ok(match n.as_str() {
        "fixe" | "steady" => Effect::Steady,
        "cycle" | "couleurs" => Effect::ColorCycle,
        "respiration" | "breathing" => Effect::Breathing,
        "vague" | "wave" => Effect::Wave,
        "onde" | "ripple" => Effect::Ripple,
        "scintillement" | "sparkle" => Effect::Sparkle,
        "trainee" | "ghosting" => Effect::Ghosting,
        "pluie" | "raindrop" => Effect::Raindrop,
        "confettis" | "confetti" => Effect::Confetti,
        "omenx" | "omen-x" => Effect::OmenX,
        "soleil" | "sun" | "balayage" | "swipe" => bail!("effet non supporté par ce clavier"),
        other => bail!("effet inconnu : {other}"),
    })
}

fn parse_color(s: &str) -> Result<Rgb> {
    Rgb::parse(s).ok_or_else(|| anyhow::anyhow!("couleur invalide : {s} (nom ou RRGGBB)"))
}

fn main() -> Result<()> {
    let args = Args::parse();
    match args.command {
        None | Some(Cmd::Gui) => omen_gui::run().map_err(|e| anyhow::anyhow!("GUI error: {e}")),
        Some(command) => {
            if run_local(&command)? {
                Ok(())
            } else {
                run_cli(command)
            }
        }
    }
}

/// Commands that need no BIOS access (keyboard HID, PDH sensors, autostart).
/// Returns `false` when the command goes to the BIOS path.
fn run_local(command: &Cmd) -> Result<bool> {
    match command {
        Cmd::KbdSet { color, intensite } => {
            let c = parse_color(color)?;
            let k = u16::from((*intensite).clamp(5, 100));
            let f = |v: u8| ((u16::from(v) * k + 50) / 100) as u8;
            let mut kb = Keyboard::open()?;
            kb.set_color(Rgb::new(f(c.r), f(c.g), f(c.b)), 100)?;
            println!("Clavier : {} appliqué ({} %)", c.hex(), intensite);
        }
        Cmd::KbdEffect {
            effect,
            vitesse,
            couleurs,
            theme,
            direction,
        } => {
            let colors = match couleurs {
                Some(list) => list.split(',').map(parse_color).collect::<Result<Vec<_>>>()?,
                None => Vec::new(),
            };
            let theme = match theme.to_lowercase().as_str() {
                "volcan" | "volcano" => Theme::Volcano,
                "jungle" => Theme::Jungle,
                "ocean" | "océan" => Theme::Ocean,
                _ => Theme::Galaxy,
            };
            let direction = match direction.to_lowercase().as_str() {
                "droite-gauche" => Direction::RightToLeft,
                "haut" => Direction::Up,
                "bas" => Direction::Down,
                "centre" => Direction::Inward,
                "depuis-centre" => Direction::Outward,
                _ => Direction::LeftToRight,
            };
            let settings = EffectSettings {
                effect: parse_effect(effect)?,
                speed: (*vitesse).min(2),
                direction,
                colors,
                theme,
                brightness: 100,
            };
            Keyboard::open()?.apply_effect(&settings)?;
            println!("Clavier : effet « {} » lancé", settings.effect.label());
        }
        Cmd::KbdOff => {
            Keyboard::open()?.turn_off()?;
            println!("Clavier : rétroéclairage éteint");
        }
        Cmd::KbdReset => {
            Keyboard::open()?.restore_default()?;
            println!("Clavier : éclairage d'usine restauré");
        }
        Cmd::KbdInfo => {
            let mut kb = Keyboard::open()?;
            println!("Clavier : {} ({})", kb.model_name(), kb.path());
            match kb.device_info()? {
                Some(i) => println!(
                    "firmware {} · type {} · langue {} · profil {} · effet {} · luminosité {}",
                    i.firmware_string(),
                    i.device_type,
                    i.language,
                    i.profile,
                    i.effect,
                    i.brightness
                ),
                None => println!("(pas de réponse GetDeviceInfo)"),
            }
            if let Some(raw) = kb.read_effect()? {
                let hex: Vec<String> = raw.iter().take(24).map(|b| format!("{b:02X}")).collect();
                println!("effet courant (0x83) : {}", hex.join(" "));
            }
        }
        Cmd::Autostart { state } => match state.to_lowercase().as_str() {
            "on" => {
                autostart::enable().map_err(anyhow::Error::msg)?;
                println!("Démarrage automatique activé (tâche « {} »)", autostart::TASK_NAME);
            }
            "off" => {
                autostart::disable().map_err(anyhow::Error::msg)?;
                println!("Démarrage automatique désactivé");
            }
            _ => println!(
                "Démarrage automatique : {}",
                if autostart::is_enabled() { "activé" } else { "désactivé" }
            ),
        },
        Cmd::Sensors => sensors(),
        _ => return Ok(false),
    }
    Ok(true)
}

fn opt(v: Option<f32>, unit: &str) -> String {
    v.map(|x| format!("{x:.0}{unit}")).unwrap_or_else(|| "—".into())
}

fn sensors() {
    let mut pdh = omen_core::sensors::PdhSensors::new();
    std::thread::sleep(std::time::Duration::from_millis(1000));
    if let Some(p) = pdh.as_mut().map(|p| p.sample()) {
        println!(
            "CPU : charge {} · zone thermique ACPI {}",
            opt(p.cpu_load, " %"),
            opt(p.thermal_zone, " °C")
        );
        println!("GPU (charge max, toutes cartes) : {}", opt(p.gpu_load, " %"));
    }
    if let Some(n) = omen_core::sensors::Nvml::load() {
        let g = n.sample();
        println!("{} : {} · {}", n.name, opt(g.temp, " °C"), opt(g.power_w, " W"));
    }
    let p = omen_core::sensors::power_state();
    println!(
        "Alimentation : {} · batterie {}",
        if p.on_ac { "secteur" } else { "batterie" },
        p.battery_percent.map(|b| format!("{b} %")).unwrap_or_else(|| "—".into())
    );
    println!(
        "Jeu détecté : {}",
        omen_core::games::detect(&[], &[]).unwrap_or_else(|| "aucun".into())
    );
    match BiosClient::connect() {
        Ok(bios) => {
            for (name, s) in [
                ("IR", Sensor::Ir),
                ("ambiant", Sensor::Ambient),
                ("PCH", Sensor::Pch),
                ("VR", Sensor::Vr),
            ] {
                match bios.get_sensor(s) {
                    Ok((0, t)) => println!("Capteur BIOS {name} : {t} °C"),
                    Ok((c, _)) => println!("Capteur BIOS {name} : code {c}"),
                    Err(e) => {
                        println!("Capteurs BIOS : {}", admin_hint(&e));
                        return;
                    }
                }
            }
            if let Ok((0, f)) = bios.get_fan_level() {
                println!(
                    "Ventilateurs : CPU {} tr/min · GPU {} tr/min",
                    omen_wmi::level_to_rpm(f.cpu),
                    omen_wmi::level_to_rpm(f.gpu)
                );
            }
        }
        Err(e) => println!("Capteurs BIOS : {}", admin_hint(&e)),
    }
}

/// Re-runs the same command line elevated (UAC prompt) in a console that
/// stays open, so the output remains readable.
fn relaunch_elevated() -> bool {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let Ok(exe) = std::env::current_exe() else { return false };
    let args: Vec<String> = std::env::args()
        .skip(1)
        .map(|a| if a.contains(' ') { format!("\"{a}\"") } else { a })
        .collect();
    let params = format!("/k \"\"{}\" {}\"", exe.display(), args.join(" "));
    let r = unsafe {
        ShellExecuteW(
            None,
            &HSTRING::from("runas"),
            &HSTRING::from("cmd.exe"),
            &HSTRING::from(params),
            None,
            SW_SHOWNORMAL,
        )
    };
    r.0 as isize > 32
}

fn run_cli(command: Cmd) -> Result<()> {
    // The WMI connection opens without admin rights; method calls do not.
    let bios = match BiosClient::connect().and_then(|b| b.get_system().map(|_| b)) {
        Ok(b) => b,
        Err(e) => {
            if !omen_core::controller::is_elevated() {
                eprintln!("Cette commande pilote le BIOS : droits administrateur nécessaires.");
                if relaunch_elevated() {
                    eprintln!("Relance en administrateur dans une nouvelle fenêtre…");
                    return Ok(());
                }
            }
            bail!("interface BIOS HP : {}", admin_hint(&e))
        }
    };

    match command {
        Cmd::Gui
        | Cmd::Sensors
        | Cmd::KbdSet { .. }
        | Cmd::KbdEffect { .. }
        | Cmd::KbdOff
        | Cmd::KbdReset
        | Cmd::KbdInfo
        | Cmd::Autostart { .. } => unreachable!("handled before the BIOS connection"),
        Cmd::Mode { mode } => {
            let level = match mode.to_lowercase().as_str() {
                "silencieux" | "silent" | "quiet" => Level::Silent,
                "equilibre" | "équilibré" | "balanced" | "default" => Level::Balanced,
                "performance" | "perf" => Level::Performance,
                "max" => Level::Max,
                other => bail!("mode inconnu : {other} (silencieux | equilibre | performance | max)"),
            };
            let (code, system) = bios.get_system()?;
            check(code, "get system data")?;
            check(bios.set_max_fan(level.max_fan())?, "max fan")?;
            check(
                bios.set_performance_profile(level.profile(), system.thermal_policy, true)?,
                "thermal mode",
            )?;
            let (_, current) = bios.get_gpu_power()?;
            let (tgp, ppab) = level.gpu_power();
            check(
                bios.set_gpu_power(GpuPowerData {
                    custom_tgp: u8::from(tgp),
                    ppab: u8::from(ppab),
                    d_state: if current.d_state == 0 { 1 } else { current.d_state },
                    peak_temperature: if current.peak_temperature == 0 {
                        87
                    } else {
                        current.peak_temperature
                    },
                })?,
                "gpu power",
            )?;
            let (_, g) = bios.get_gpu_power()?;
            let (_, max) = bios.get_max_fan()?;
            println!(
                "Mode {} appliqué (mode thermique 0x{:02X}, TGP {}, Boost {}, ventilateurs max {})",
                level.label(),
                level.profile().bios_mode(system.thermal_policy),
                g.custom_tgp,
                g.ppab,
                if max { "oui" } else { "non" }
            );
        }
        Cmd::Info => {
            let (code, system) = bios.get_system()?;
            check(code, "get system data")?;
            println!("System design data");
            println!("  status flags          : 0x{:04X}", system.status_flags);
            println!("  thermal policy        : V{}", system.thermal_policy);
            println!(
                "  software fan control  : {}",
                if system.sw_fan_control() { "yes" } else { "no" }
            );
            println!(
                "  extreme mode          : {}",
                if system.extreme_mode() { "yes" } else { "no" }
            );
            println!("  default PL4           : {} W", system.default_cpu_pl4);
            println!("  default PL w/ GPU     : {} W", system.default_cpu_power_with_gpu);
            println!(
                "  BIOS overclocking     : {}",
                if system.bios_oc == 1 { "yes" } else { "no" }
            );
            println!("  GPU mode switch flags : 0x{:02X}", system.gpu_mode_switch);

            let (code, info) = bios.get_fan_type_info()?;
            check(code, "get fan type")?;
            println!("Fans");
            println!("  count                 : {}", info.fan_count());
            println!("  types                 : {:?}", info.types);
            println!("  capabilities          : 0x{:04X}", info.capabilities);

            if let Ok((0, a)) = bios.get_adapter_status() {
                println!("Adapter                 : {} ({} W)", a.label(), a.watts);
            }

            let (code, gpu) = bios.get_gpu_power()?;
            check(code, "get gpu power")?;
            println!("GPU power");
            println!(
                "  custom TGP            : {}",
                if gpu.custom_tgp == 1 { "on" } else { "off" }
            );
            println!("  PPAB                  : {}", if gpu.ppab == 1 { "on" } else { "off" });
            println!("  device state          : D{}", gpu.d_state);
            println!("  peak temperature      : {} C", gpu.peak_temperature);
        }
        Cmd::ConnectTest => {
            println!("{}", omen_wmi::connection_report());
            println!("{}", omen_wmi::put_lab());
        }
        Cmd::FanTable => {
            let (code, ft) = bios.get_fan_table()?;
            check(code, "fan table")?;
            println!("BIOS fan table: {} fans, {} levels", ft.fan_count, ft.level_count);
            for (i, e) in ft.entries.iter().enumerate() {
                println!(
                    "{i:02}: {} C -> cpu 0x{:02X} ({} rpm) / gpu 0x{:02X} ({} rpm)",
                    e.temp_c,
                    e.cpu,
                    omen_wmi::level_to_rpm(e.cpu),
                    e.gpu,
                    omen_wmi::level_to_rpm(e.gpu)
                );
            }
        }
        Cmd::Kbd => {
            println!("note: these BIOS registers do not drive the per-key RGB LEDs; use kbd-set / kbd-effect");
            let (code, support) = bios.has_backlight()?;
            println!("backlight support: {support} (code {code})");
            let (_, kbd) = bios.get_kbd_type()?;
            println!("keyboard type: {kbd:?}");
            let (_, bl) = bios.get_backlight()?;
            println!("backlight register: {bl:?}");
            let (_, ct) = bios.get_color_table()?;
            println!("color table: {} zones", ct.zones.len());
            print_color_table(&ct);
        }
        Cmd::Backlight { state } => match state.to_lowercase().as_str() {
            "status" => {
                let (_, bl) = bios.get_backlight()?;
                println!("backlight register: {bl:?}");
            }
            "on" => {
                check(bios.set_backlight(Backlight::On)?, "set backlight")?;
                println!("backlight register ON (the RGB keyboard itself: omen kbd-set)");
            }
            "off" => {
                check(bios.set_backlight(Backlight::Off)?, "set backlight")?;
                println!("backlight register OFF (the RGB keyboard itself: omen kbd-off)");
            }
            other => bail!("expected on|off|status, got {other}"),
        },
        Cmd::Color4 { colors } => match colors {
            None => {
                let (code, ct) = bios.get_color_table()?;
                check(code, "color table")?;
                print_color_table(&ct);
            }
            Some(spec) => {
                let table = parse_color4(&spec)?;
                check(bios.set_color_table(&table)?, "set colors")?;
                let (_, ct) = bios.get_color_table()?;
                println!("applied (BIOS register; per-key RGB models ignore it):");
                print_color_table(&ct);
            }
        },
        Cmd::GpuMode { mode, yes } => match mode {
            None => {
                let (code, m) = bios.get_gpu_mode()?;
                check(code, "get gpu mode")?;
                println!("GPU mode: {m:?}");
            }
            Some(spec) => {
                if !yes {
                    bail!("changing the GPU/MUX mode requires a reboot; re-run with --yes to confirm");
                }
                let m = match spec.to_lowercase().as_str() {
                    "hybrid" => GpuMode::Hybrid,
                    "discrete" => GpuMode::Discrete,
                    "optimus" => GpuMode::Optimus,
                    other => bail!("expected hybrid|discrete|optimus, got {other}"),
                };
                check(bios.set_gpu_mode(m)?, "set gpu mode")?;
                println!("GPU mode set to {m:?} — reboot required");
            }
        },
        Cmd::Watch { interval } => {
            let interval = interval.max(1);
            println!("Monitoring every {interval}s (Ctrl+C to stop)");
            let mut pdh = omen_core::sensors::PdhSensors::new();
            loop {
                let (_, ir) = bios.get_sensor(Sensor::Ir)?;
                let (_, fans) = bios.get_fan_level()?;
                let (_, thr) = bios.get_throttling()?;
                let p = pdh.as_mut().map(|p| p.sample()).unwrap_or_default();
                println!(
                    "cpu={} zone={} ir={ir}C fan_cpu={}rpm fan_gpu={}rpm thr={thr:?}",
                    opt(p.cpu_load, "%"),
                    opt(p.thermal_zone, "C"),
                    omen_wmi::level_to_rpm(fans.cpu),
                    omen_wmi::level_to_rpm(fans.gpu)
                );
                std::thread::sleep(std::time::Duration::from_secs(interval));
            }
        }
        Cmd::Raw {
            cmd_type,
            out_size,
            data,
        } => {
            let ct = if let Some(h) = cmd_type.strip_prefix("0x") {
                u32::from_str_radix(h, 16)
                    .map_err(|_| anyhow::anyhow!("invalid cmd type: {cmd_type}"))?
            } else {
                cmd_type
                    .parse()
                    .map_err(|_| anyhow::anyhow!("invalid cmd type: {cmd_type}"))?
            };
            let mut payload = Vec::new();
            for s in &data {
                let b = u8::from_str_radix(s.trim_start_matches("0x").trim(), 16)
                    .map_err(|_| anyhow::anyhow!("invalid byte: {s}"))?;
                payload.push(b);
            }
            let payload_ref = if payload.is_empty() { None } else { Some(payload.as_slice()) };
            let (code, out) = bios.send(omen_wmi::CMD_DEFAULT, ct, payload_ref, out_size)?;
            println!("rwReturnCode: {code}");
            let hex: Vec<String> = out.iter().map(|b| format!("{b:02X}")).collect();
            println!("out ({} bytes): {}", hex.len(), hex.join(" "));
        }
        Cmd::Fans => {
            let (code, levels) = bios.get_fan_level()?;
            check(code, "get fan level")?;
            println!(
                "CPU fan: {} rpm (0x{:02X}), GPU fan: {} rpm (0x{:02X})",
                omen_wmi::level_to_rpm(levels.cpu),
                levels.cpu,
                omen_wmi::level_to_rpm(levels.gpu),
                levels.gpu
            );
            let (code, max) = bios.get_max_fan()?;
            check(code, "get max fan")?;
            println!("Max fan mode : {}", if max { "ON" } else { "off" });
        }
        Cmd::Temp => {
            for (name, s) in [
                ("IR", Sensor::Ir),
                ("ambient", Sensor::Ambient),
                ("PCH", Sensor::Pch),
                ("VR", Sensor::Vr),
            ] {
                let (code, t) = bios.get_sensor(s)?;
                println!("{name:<8}: {t} C (code {code})");
            }
            let (code, throttling) = bios.get_throttling()?;
            check(code, "get throttling")?;
            let label = match throttling {
                Throttling::Unknown => "unknown".to_string(),
                Throttling::On => "throttling".to_string(),
                Throttling::Default => "default (no throttling)".to_string(),
                Throttling::Other(v) => format!("other (0x{v:02X})"),
            };
            println!("Throttling: {label}");
        }
        Cmd::Power => {
            let (code, data) = bios.get_power_data()?;
            check(code, "get power data")?;
            let hex: Vec<String> = data.iter().take(32).map(|b| format!("{b:02X}")).collect();
            println!("Raw 0x2A power data: {}", hex.join(" "));
        }
        Cmd::GpuPower => {
            let (code, gpu) = bios.get_gpu_power()?;
            check(code, "get gpu power")?;
            println!(
                "custom_tgp={} ppab={} d_state={} peak_temperature={}",
                gpu.custom_tgp, gpu.ppab, gpu.d_state, gpu.peak_temperature
            );
        }
        Cmd::MaxFan { state } => {
            let on = match state.to_lowercase().as_str() {
                "on" => true,
                "off" => false,
                other => bail!("expected on|off, got {other}"),
            };
            check(bios.set_max_fan(on)?, "set max fan")?;
            println!("Max fan mode set to {}", if on { "ON" } else { "off" });
        }
        Cmd::FanMode { mode } => {
            let mode: FanMode = mode.parse().map_err(anyhow::Error::msg)?;
            check(bios.set_fan_mode(mode)?, "set fan mode")?;
            println!("Fan mode set to {mode}");
        }
        Cmd::FanLevel { cpu, gpu } => {
            check(bios.set_fan_level(cpu, gpu)?, "set fan level")?;
            println!(
                "Fan levels set to CPU {} rpm / GPU {} rpm",
                omen_wmi::level_to_rpm(cpu),
                omen_wmi::level_to_rpm(gpu)
            );
        }
        Cmd::GpuSet {
            custom_tgp,
            ppab,
            d_state,
            peak_temperature,
        } => {
            let data = GpuPowerData {
                custom_tgp,
                ppab,
                d_state,
                peak_temperature,
            };
            check(bios.set_gpu_power(data)?, "set gpu power")?;
            println!("GPU power data written: {data:?}");
        }
        Cmd::CpuSet {
            pl1,
            pl2,
            pl4,
            with_gpu,
        } => {
            let data = CpuPowerData {
                limit1: pl1,
                limit2: pl2,
                limit4: pl4,
                limit_with_gpu: with_gpu,
            };
            check(bios.set_cpu_power(data)?, "set cpu power")?;
            println!("CPU power limits written (accepted by BIOS; no readback available on this model): {data:?}");
        }
    }

    Ok(())
}
