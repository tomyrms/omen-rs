//! Persistent user settings (`%APPDATA%\omen-rs\config.json`).

use std::path::PathBuf;

use omen_kbd::{EffectSettings, Rgb};
use omen_wmi::FanCurve;
use serde::{Deserialize, Serialize};

use crate::mode::Mode;

/// How eagerly the automatic mode reacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Sensitivity {
    /// Favors silence: higher thresholds, slower to speed up.
    Calm,
    Normal,
    /// Favors cooling: lower thresholds, faster to speed up.
    Reactive,
}

impl Sensitivity {
    pub fn label(self) -> &'static str {
        match self {
            Sensitivity::Calm => "Discret",
            Sensitivity::Normal => "Normal",
            Sensitivity::Reactive => "Réactif",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoSettings {
    pub sensitivity: Sensitivity,
    /// Switch to Performance while a game runs.
    pub performance_for_games: bool,
    /// Only boost for games when plugged in (like the vendor application).
    pub games_only_on_ac: bool,
    /// Go to maximum fans when the machine gets too hot.
    pub max_fans_when_hot: bool,
    /// Drop to the silent profile when the machine is idle.
    pub silent_when_idle: bool,
    /// Extra executables (file names, e.g. `game.exe`) treated as games.
    pub extra_games: Vec<String>,
    /// Executables never treated as games.
    pub ignored_games: Vec<String>,
}

impl Default for AutoSettings {
    fn default() -> Self {
        Self {
            sensitivity: Sensitivity::Normal,
            performance_for_games: true,
            games_only_on_ac: false,
            max_fans_when_hot: true,
            silent_when_idle: true,
            extra_games: Vec::new(),
            ignored_games: Vec::new(),
        }
    }
}

/// What the keyboard should show; reapplied at startup.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum KeyboardLook {
    Color { color: Rgb, brightness: u8 },
    Effect(EffectSettings),
    Off,
}

impl Default for KeyboardLook {
    fn default() -> Self {
        KeyboardLook::Color {
            color: Rgb::new(255, 40, 40),
            brightness: 100,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub mode: Mode,
    pub auto: AutoSettings,
    pub custom_curve: FanCurve,
    pub keyboard: KeyboardLook,
    /// Reapply the keyboard look when the app starts.
    pub restore_keyboard_on_start: bool,
    /// Start hidden in the notification area.
    pub start_minimized: bool,
    /// Closing the window keeps the app running in the notification area.
    pub close_to_tray: bool,
    /// Read the NVIDIA GPU sensors while on battery (keeps the GPU awake).
    pub gpu_sensors_on_battery: bool,
    pub custom_colors: Vec<Rgb>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            mode: Mode::Auto,
            auto: AutoSettings::default(),
            custom_curve: FanCurve::default_curve(),
            keyboard: KeyboardLook::default(),
            restore_keyboard_on_start: false,
            start_minimized: false,
            close_to_tray: true,
            gpu_sensors_on_battery: false,
            custom_colors: Vec::new(),
        }
    }
}

pub fn data_dir() -> PathBuf {
    let base = std::env::var("APPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("omen-rs")
}

fn path() -> PathBuf {
    data_dir().join("config.json")
}

impl Config {
    pub fn load() -> Self {
        std::fs::read_to_string(path())
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let p = path();
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            // Write then rename so a crash never leaves a truncated file.
            let tmp = p.with_extension("json.tmp");
            if std::fs::write(&tmp, json).is_ok() {
                let _ = std::fs::rename(&tmp, &p);
            }
        }
    }
}

/// Appends a line to `%APPDATA%\omen-rs\omen-hub.log` (rotated at 1 MB).
pub fn log_line(msg: &str) {
    use std::io::Write as _;
    let p = data_dir().join("omen-hub.log");
    let _ = std::fs::create_dir_all(data_dir());
    if std::fs::metadata(&p).map(|m| m.len() > 1_000_000).unwrap_or(false) {
        let _ = std::fs::rename(&p, p.with_extension("log.old"));
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&p) {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(f, "[{ts}] {msg}");
    }
}
