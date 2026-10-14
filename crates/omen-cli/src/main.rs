use anyhow::{bail, Result};
use clap::{Parser, Subcommand};
use omen_wmi::{
    Backlight, BiosClient, ColorTable, ColorZone, CpuPowerData, FanMode, GpuMode, GpuPowerData,
    Throttling,
};

#[derive(Parser)]
#[command(
    name = "omen",
    about = "OMEN hardware control (Rust rewrite of the OMEN Gaming Hub control path). No subcommand: launches the GUI.",
    version
)]
struct Args {
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Show system capabilities, fans, GPU power and thermal state
    Info,
    /// Diagnose the WMI connection step by step (run elevated)
    ConnectTest,
    /// Show the BIOS fan table (0x2F)
    FanTable,
    /// Keyboard: type, backlight, color zones
    Kbd,
    /// Keyboard backlight: on | off | status
    Backlight {
        /// on | off | status
        state: String,
    },
    /// Keyboard zone colors: "RRGGBB:RRGGBB:..." (1-4 zones), or read current
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
    /// Read the thermal sensor (degrees Celsius)
    Temp,
    /// Read raw 0x2A power data
    Power,
    /// Show GPU power state (custom TGP / PPAB)
    #[command(alias = "gpupower")]
    GpuPower,
    /// Enable or disable maximum fan speed
    #[command(alias = "maxfan")]
    MaxFan {
        /// on | off
        state: String,
    },
    /// Set the fan performance mode
    #[command(alias = "fanmode")]
    FanMode {
        /// default | performance | cool | l0 .. l6 | legacy-default | legacy-performance | legacy-cool | legacy-quiet | legacy-extreme
        mode: String,
    },
    /// Set manual fan levels (raw units, observed range 0x15..0x39)
    #[command(alias = "fanlevel")]
    FanLevel {
        cpu: u8,
        gpu: u8,
    },
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
        let v = u32::from_str_radix(part.trim().trim_start_matches('#'), 16)
            .map_err(|_| anyhow::anyhow!("invalid color: {part}"))?;
        zones.push(ColorZone {
            r: ((v >> 16) & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            b: (v & 0xFF) as u8,
        });
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

fn main() -> Result<()> {
    let args = Args::parse();

    match args.command {
        None | Some(Cmd::Gui) => {
            omen_gui::run().map_err(|e| anyhow::anyhow!("GUI error: {e}"))
        }
        Some(command) => run_cli(command),
    }
}

fn run_cli(command: Cmd) -> Result<()> {
    let bios = BiosClient::connect()?;

    match command {
        // Handled in main() before any WMI connection is made.
        Cmd::Gui => unreachable!("GUI is launched in main()"),
        Cmd::Info => {
            let (code, system) = bios.get_system()?;
            check(code, "get system data")?;
            println!("System design data");
            println!("  status flags          : 0x{:04X}", system.status_flags);
            println!(
                "  thermal policy        : V{}",
                system.thermal_policy
            );
            println!(
                "  software fan control  : {}",
                if system.sw_fan_control() { "yes" } else { "no" }
            );
            println!(
                "  extreme mode          : {}",
                if system.extreme_mode() { "yes" } else { "no" }
            );
            println!(
                "  default PL4           : {} W",
                system.default_cpu_pl4
            );
            println!(
                "  default PL w/ GPU     : {} W",
                system.default_cpu_power_with_gpu
            );
            println!(
                "  BIOS overclocking     : {}",
                if system.bios_oc == 1 { "yes" } else { "no" }
            );
            println!("  GPU mode switch flags : 0x{:02X}", system.gpu_mode_switch);

            let (code, count) = bios.get_fan_count()?;
            check(code, "get fan count")?;
            let (code, fan_type) = bios.get_fan_type()?;
            check(code, "get fan type")?;
            println!("Fans");
            println!("  count                 : {count}");
            println!(
                "  types                 : fan1=0x{:X} fan2=0x{:X}",
                fan_type & 0x0F,
                (fan_type >> 4) & 0x0F
            );

            let (code, gpu) = bios.get_gpu_power()?;
            check(code, "get gpu power")?;
            println!("GPU power");
            println!(
                "  custom TGP            : {}",
                if gpu.custom_tgp == 1 { "on" } else { "off" }
            );
            println!(
                "  PPAB                  : {}",
                if gpu.ppab == 1 { "on" } else { "off" }
            );
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
            println!(
                "BIOS fan table: {} fans, {} levels",
                ft.fan_count, ft.level_count
            );
            for (i, e) in ft.entries.iter().enumerate() {
                println!(
                    "{i:02}: {} C -> cpu 0x{:02X} / gpu 0x{:02X}",
                    e.temp_c, e.cpu, e.gpu
                );
            }
        }
        Cmd::Kbd => {
            let (code, support) = bios.has_backlight()?;
            println!("backlight support: {support} (code {code})");
            let (code, raw) = bios.send(
                omen_wmi::CMD_KEYBOARD,
                omen_wmi::cmd::GET_BACKLIGHT,
                Some(&[0, 0, 0, 0]),
                4,
            )?;
            println!("backlight raw   : code={code} bytes={raw:02X?}");
            let (_, kbd) = bios.get_kbd_type()?;
            println!("keyboard type: {kbd:?}");
            let (_, bl) = bios.get_backlight()?;
            println!("backlight: {bl:?}");
            let (_, ct) = bios.get_color_table()?;
            println!("color table: {} zones", ct.zones.len());
            print_color_table(&ct);
        }
        Cmd::Backlight { state } => match state.to_ascii_lowercase().as_str() {
            "status" => {
                let (_, bl) = bios.get_backlight()?;
                println!("backlight: {bl:?}");
            }
            "on" => {
                let code = bios.set_backlight(Backlight::On)?;
                check(code, "set backlight")?;
                println!("backlight ON");
            }
            "off" => {
                let code = bios.set_backlight(Backlight::Off)?;
                check(code, "set backlight")?;
                println!("backlight OFF");
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
                let code = bios.set_color_table(&table)?;
                check(code, "set colors")?;
                let (_, ct) = bios.get_color_table()?;
                println!("applied:");
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
                    bail!(
                        "changing the GPU/MUX mode requires a reboot; re-run with --yes to confirm"
                    );
                }
                let m = match spec.to_ascii_lowercase().as_str() {
                    "hybrid" => GpuMode::Hybrid,
                    "discrete" => GpuMode::Discrete,
                    "optimus" => GpuMode::Optimus,
                    other => bail!("expected hybrid|discrete|optimus, got {other}"),
                };
                let code = bios.set_gpu_mode(m)?;
                check(code, "set gpu mode")?;
                println!("GPU mode set to {m:?} — reboot required");
            }
        },
        Cmd::Watch { interval } => {
            let interval = interval.max(1);
            println!("Monitoring every {interval}s (Ctrl+C to stop)");
            loop {
                let (_, temp) = bios.get_temperature()?;
                let (_, fans) = bios.get_fan_level()?;
                let (_, thr) = bios.get_throttling()?;
                let (_, raw) = bios.get_power_data()?;
                let hex: Vec<String> = raw.iter().take(8).map(|b| format!("{b:02X}")).collect();
                println!(
                    "temp={temp}C fan_cpu=0x{:02X} fan_gpu=0x{:02X} thr={thr:?} 2A=[{}]",
                    fans.cpu,
                    fans.gpu,
                    hex.join(" ")
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
                u32::from_str_radix(h, 16).map_err(|_| anyhow::anyhow!("invalid cmd type: {cmd_type}"))?
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
            let payload_ref = if payload.is_empty() {
                None
            } else {
                Some(payload.as_slice())
            };
            let (code, out) = bios.send(omen_wmi::CMD_DEFAULT, ct, payload_ref, out_size)?;
            println!("rwReturnCode: {code}");
            let hex: Vec<String> = out.iter().map(|b| format!("{b:02X}")).collect();
            println!("out ({} bytes): {}", hex.len(), hex.join(" "));
        }
        Cmd::Fans => {
            let (code, levels) = bios.get_fan_level()?;
            check(code, "get fan level")?;
            println!(
                "CPU fan level: 0x{:02X} (~{} %), GPU fan level: 0x{:02X} (~{} %)",
                levels.cpu,
                levels.cpu_percent(),
                levels.gpu,
                levels.gpu_percent()
            );
            let (code, max) = bios.get_max_fan()?;
            check(code, "get max fan")?;
            println!("Max fan mode : {}", if max { "ON" } else { "off" });
        }
        Cmd::Temp => {
            let (code, temp) = bios.get_temperature()?;
            check(code, "get temperature")?;
            println!("Thermal sensor: {temp} C");
            let (code, throttling) = bios.get_throttling()?;
            check(code, "get throttling")?;
            let label = match throttling {
                Throttling::Unknown => "unknown",
                Throttling::On => "throttling",
                Throttling::Default => "default (no throttling)",
                Throttling::Other(v) => return Ok(println!("Throttling: other (0x{v:02X})")),
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
            let on = match state.to_ascii_lowercase().as_str() {
                "on" => true,
                "off" => false,
                other => bail!("expected on|off, got {other}"),
            };
            let code = bios.set_max_fan(on)?;
            check(code, "set max fan")?;
            println!("Max fan mode set to {}", if on { "ON" } else { "off" });
        }
        Cmd::FanMode { mode } => {
            let mode: FanMode = mode.parse().map_err(anyhow::Error::msg)?;
            let code = bios.set_fan_mode(mode)?;
            check(code, "set fan mode")?;
            println!("Fan mode set to {mode}");
        }
        Cmd::FanLevel { cpu, gpu } => {
            let code = bios.set_fan_level(cpu, gpu)?;
            check(code, "set fan level")?;
            println!("Fan levels set to CPU=0x{cpu:02X} GPU=0x{gpu:02X}");
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
            let code = bios.set_gpu_power(data)?;
            check(code, "set gpu power")?;
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
            let code = bios.set_cpu_power(data)?;
            check(code, "set cpu power")?;
            println!("CPU power limits written: {data:?}");
        }
    }

    Ok(())
}
