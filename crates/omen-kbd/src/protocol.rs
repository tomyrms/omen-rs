//! Report builders for the keyboard MCU "general" command set.
//!
//! Every report is 65 bytes: `[0]` HID report id (0), `[1]` command,
//! `[2]` parameter/target, `[3]` data length, `[4]` reserved, `[5..]` data.
//! The MCU answers each command with a report echoing `[1]`/`[2]`; bytes
//! `[5] [6]` = `EC AC` mean accepted, `EC FA` means rejected.
//!
//! Facts (command codes, layouts, value ranges) come from the protocol as
//! observed in the vendor SDK on the target machine; this is an original
//! implementation of that wire format.

use serde::{Deserialize, Serialize};

pub const REPORT_LEN: usize = 65;
/// Size of a per-key color map (one byte per LED index).
pub const KEYMAP_LEN: usize = 144;
const PAGE_LEN: usize = 60;

pub type Report = [u8; REPORT_LEN];

pub mod cmd {
    pub const SET_EFFECT: u8 = 0x03;
    pub const SET_KEYS_R: u8 = 0x05;
    pub const SET_KEYS_G: u8 = 0x06;
    pub const SET_KEYS_B: u8 = 0x07;
    pub const LIGHTING_ON_OFF: u8 = 0x09;
    pub const STORE_TO_FLASH: u8 = 0x0A;
    pub const BRIGHTNESS: u8 = 0x0C;
    pub const RESTORE_DEFAULT: u8 = 0x10;
    pub const GET_INFO: u8 = 0x80;
    pub const GET_EFFECT: u8 = 0x83;
}

fn report(command: u8, param: u8, len: u8, data: &[u8]) -> Report {
    let mut r = [0u8; REPORT_LEN];
    r[1] = command;
    r[2] = param;
    r[3] = len;
    let n = data.len().min(REPORT_LEN - 5);
    r[5..5 + n].copy_from_slice(&data[..n]);
    r
}

/// `0x80 / 1`: device information (firmware, device type, language, profile,
/// current effect, brightness).
pub fn get_device_info() -> Report {
    report(cmd::GET_INFO, 1, 0, &[])
}

/// `0x09`: keyboard lighting on (1) / off (0).
pub fn lighting(on: bool) -> Report {
    report(cmd::LIGHTING_ON_OFF, 0, 1, &[u8::from(on)])
}

/// `0x0C`: brightness as a physical percentage (encoded as 100 + percent).
pub fn brightness(percent: u8) -> Report {
    report(cmd::BRIGHTNESS, 0, 1, &[100 + percent.min(100)])
}

/// `0x05/0x06/0x07` x 3 pages: one color channel per command, 60 LEDs per
/// page (the last page carries the remaining 24 of the 144 LEDs).
pub fn static_keymaps(r: &[u8; KEYMAP_LEN], g: &[u8; KEYMAP_LEN], b: &[u8; KEYMAP_LEN]) -> Vec<Report> {
    let mut out = Vec::with_capacity(9);
    for (command, map) in [(cmd::SET_KEYS_R, r), (cmd::SET_KEYS_G, g), (cmd::SET_KEYS_B, b)] {
        for page in 0..3 {
            let start = page * PAGE_LEN;
            let end = (start + PAGE_LEN).min(KEYMAP_LEN);
            out.push(report(command, page as u8, 0, &map[start..end]));
        }
    }
    out
}

/// `0x0A`: persist the current lighting in the MCU flash (target 0 = all LEDs).
pub fn store_to_flash() -> Report {
    report(cmd::STORE_TO_FLASH, 0, 2, &[0xAC, 0x53])
}

/// `0x10 / 7`: restore the factory lighting settings.
pub fn restore_lighting_default() -> Report {
    report(cmd::RESTORE_DEFAULT, 7, 4, &[0x94, 0x10, 0x98, 0x27])
}

/// `0x83`: read back the active lighting effect.
pub fn get_effect() -> Report {
    report(cmd::GET_EFFECT, 0, 0, &[])
}

/// Animated effects supported by the keyboard MCU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Effect {
    Steady,
    ColorCycle,
    Sparkle,
    Breathing,
    Ghosting,
    Ripple,
    Wave,
    OmenX,
    Raindrop,
    Confetti,
    Sun,
    Swipe,
}

impl Effect {
    /// Effects this keyboard firmware accepts (verified on 0.3.21.0: Steady,
    /// Sun and Swipe are refused whatever the parameters; a plain color goes
    /// through the per-key maps instead).
    pub const ALL: [Effect; 9] = [
        Effect::ColorCycle,
        Effect::Breathing,
        Effect::Wave,
        Effect::Ripple,
        Effect::Sparkle,
        Effect::Ghosting,
        Effect::Raindrop,
        Effect::Confetti,
        Effect::OmenX,
    ];

    /// Firmware-side support (see [`Effect::ALL`]).
    pub fn supported(self) -> bool {
        Self::ALL.contains(&self)
    }

    /// Only these accept a built-in theme; the others need explicit colors.
    pub fn takes_theme(self) -> bool {
        matches!(self, Effect::ColorCycle | Effect::Wave)
    }

    pub fn code(self) -> u8 {
        match self {
            Effect::Steady => 1,
            Effect::Breathing => 2,
            Effect::ColorCycle => 4,
            Effect::Sparkle => 7,
            Effect::Ghosting => 8,
            Effect::Ripple => 9,
            Effect::Wave => 10,
            Effect::Raindrop => 12,
            Effect::OmenX => 13,
            Effect::Confetti => 15,
            Effect::Sun => 16,
            Effect::Swipe => 17,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Effect::Steady => "Fixe",
            Effect::ColorCycle => "Cycle de couleurs",
            Effect::Sparkle => "Scintillement",
            Effect::Breathing => "Respiration",
            Effect::Ghosting => "Traînée",
            Effect::Ripple => "Onde (au toucher)",
            Effect::Wave => "Vague",
            Effect::OmenX => "OMEN X",
            Effect::Raindrop => "Gouttes de pluie",
            Effect::Confetti => "Confettis",
            Effect::Sun => "Soleil",
            Effect::Swipe => "Balayage",
        }
    }

    /// Whether the effect uses the direction setting.
    pub fn has_direction(self) -> bool {
        matches!(self, Effect::Wave | Effect::Swipe)
    }
}

/// Built-in color themes (used when no custom colors are given).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Theme {
    Volcano,
    Jungle,
    Ocean,
    Galaxy,
}

impl Theme {
    pub const ALL: [Theme; 4] = [Theme::Galaxy, Theme::Volcano, Theme::Jungle, Theme::Ocean];

    fn code(self) -> u8 {
        match self {
            Theme::Volcano => 2,
            Theme::Jungle => 3,
            Theme::Ocean => 4,
            Theme::Galaxy => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Theme::Volcano => "Volcan",
            Theme::Jungle => "Jungle",
            Theme::Ocean => "Océan",
            Theme::Galaxy => "Galaxie",
        }
    }

    /// Colors standing in for the theme on effects that refuse themes.
    pub fn palette(self) -> Vec<Rgb> {
        let c = |r, g, b| Rgb::new(r, g, b);
        match self {
            Theme::Volcano => vec![c(255, 0, 0), c(255, 80, 0), c(255, 160, 0), c(180, 0, 0)],
            Theme::Jungle => vec![c(0, 255, 0), c(120, 255, 0), c(0, 160, 40), c(200, 255, 0)],
            Theme::Ocean => vec![c(0, 80, 255), c(0, 200, 255), c(0, 255, 180), c(0, 0, 200)],
            Theme::Galaxy => vec![c(150, 0, 255), c(255, 0, 180), c(0, 80, 255), c(0, 220, 255)],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Direction {
    LeftToRight,
    RightToLeft,
    Up,
    Down,
    Inward,
    Outward,
}

impl Direction {
    pub const ALL: [Direction; 6] = [
        Direction::LeftToRight,
        Direction::RightToLeft,
        Direction::Up,
        Direction::Down,
        Direction::Inward,
        Direction::Outward,
    ];

    fn code(self) -> u8 {
        match self {
            Direction::Inward => 0,
            Direction::Outward => 1,
            Direction::RightToLeft => 2,
            Direction::LeftToRight => 3,
            Direction::Up => 4,
            Direction::Down => 5,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Direction::LeftToRight => "Gauche vers droite",
            Direction::RightToLeft => "Droite vers gauche",
            Direction::Up => "Vers le haut",
            Direction::Down => "Vers le bas",
            Direction::Inward => "Vers le centre",
            Direction::Outward => "Depuis le centre",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Parses `#RRGGBB`, `RRGGBB` or a French/English color name.
    pub fn parse(s: &str) -> Option<Self> {
        let named = match s.trim().to_ascii_lowercase().as_str() {
            "rouge" | "red" => Some(Rgb::new(255, 0, 0)),
            "vert" | "green" => Some(Rgb::new(0, 255, 0)),
            "bleu" | "blue" => Some(Rgb::new(0, 0, 255)),
            "blanc" | "white" => Some(Rgb::new(255, 255, 255)),
            "jaune" | "yellow" => Some(Rgb::new(255, 200, 0)),
            "orange" => Some(Rgb::new(255, 90, 0)),
            "violet" | "purple" => Some(Rgb::new(150, 0, 255)),
            "rose" | "pink" => Some(Rgb::new(255, 40, 140)),
            "cyan" => Some(Rgb::new(0, 220, 255)),
            "magenta" => Some(Rgb::new(255, 0, 255)),
            _ => None,
        };
        if named.is_some() {
            return named;
        }
        let hex = s.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(hex, 16).ok()?;
        Some(Rgb::new((v >> 16) as u8, (v >> 8) as u8, v as u8))
    }

    pub fn hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

/// Settings of an animated effect.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectSettings {
    pub effect: Effect,
    /// 0 slow, 1 medium, 2 fast.
    pub speed: u8,
    pub direction: Direction,
    /// Custom colors (1 to 6). Empty = use `theme`.
    pub colors: Vec<Rgb>,
    pub theme: Theme,
    /// Brightness percentage, sent with `0x0C` before the effect.
    pub brightness: u8,
}

impl Default for EffectSettings {
    fn default() -> Self {
        Self {
            effect: Effect::Wave,
            speed: 1,
            direction: Direction::LeftToRight,
            colors: Vec::new(),
            theme: Theme::Galaxy,
            brightness: 100,
        }
    }
}

/// `0x03 / target 0`, 36 data bytes: effect, show mode, color count, speed,
/// brightness, direction, ripple size, raindrop frequency, audio levels,
/// then up to six RGB triplets at data offset 24.
pub fn effect(settings: &EffectSettings) -> Report {
    let mut d = [0u8; 42];
    d[0] = settings.effect.code();
    if settings.colors.is_empty() && !settings.effect.takes_theme() {
        let palette = EffectSettings { colors: settings.theme.palette(), ..settings.clone() };
        return effect(&palette);
    }
    if settings.colors.is_empty() {
        d[1] = settings.theme.code();
        d[2] = 4;
    } else {
        let n = settings.colors.len().min(6);
        d[1] = u8::from(n > 1);
        // Color count encoding: 0..3 for 1..4 colors, then 5, 6.
        d[2] = match n {
            1..=4 => (n - 1) as u8,
            5 => 5,
            _ => 6,
        };
        for (i, c) in settings.colors.iter().take(6).enumerate() {
            d[24 + 3 * i] = c.r;
            d[25 + 3 * i] = c.g;
            d[26 + 3 * i] = c.b;
        }
    }
    d[3] = settings.speed.min(2);
    // Brightness byte left at 0 like the vendor effect path; brightness is
    // applied separately with `0x0C` (see `Keyboard::apply_effect`).
    d[4] = 0;
    d[5] = settings.direction.code();
    d[6] = 1;
    d[7] = settings.speed.min(2);
    report(cmd::SET_EFFECT, 0, 36, &d)
}

/// MCU answer to one command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Ack {
    Accepted,
    Rejected,
    /// Answered without the accept/reject marker (data replies such as info).
    Data,
}

/// Classifies a response report (including the report id byte).
pub fn classify(resp: &[u8]) -> Ack {
    match (resp.get(5), resp.get(6)) {
        (Some(0xEC), Some(0xAC)) => Ack::Accepted,
        (Some(0xEC), Some(0xFA)) => Ack::Rejected,
        _ => Ack::Data,
    }
}

/// Decoded `0x80` device information.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub firmware: [u8; 4],
    pub device_type: u8,
    pub language: u8,
    pub profile: u8,
    pub effect: u8,
    pub brightness: u8,
}

impl DeviceInfo {
    /// Parses a response report (with the report id at index 0).
    pub fn parse(resp: &[u8]) -> Option<Self> {
        let d = resp.get(1..)?;
        if d.len() < 13 || classify(resp) == Ack::Rejected {
            return None;
        }
        Some(Self {
            firmware: [d[4], d[5], d[6], d[7]],
            device_type: d[8],
            language: d[9],
            profile: d[10],
            effect: d[11],
            brightness: d[12],
        })
    }

    pub fn firmware_string(&self) -> String {
        format!(
            "{:X}.{:X}.{:X}.{:X}",
            self.firmware[0], self.firmware[1], self.firmware[2], self.firmware[3]
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_maps_are_split_in_three_pages() {
        let r = [0xFF; KEYMAP_LEN];
        let z = [0u8; KEYMAP_LEN];
        let reps = static_keymaps(&r, &z, &z);
        assert_eq!(reps.len(), 9);
        assert_eq!(&reps[0][..5], &[0, 0x05, 0, 0, 0]);
        assert_eq!(reps[0][5], 0xFF);
        assert_eq!(reps[0][64], 0xFF);
        // Third page: 24 LEDs then padding.
        assert_eq!(reps[2][2], 2);
        assert_eq!(reps[2][5 + 23], 0xFF);
        assert_eq!(reps[2][5 + 24], 0);
        assert_eq!(reps[3][1], 0x06);
    }

    #[test]
    fn brightness_is_physical_value() {
        let r = brightness(80);
        assert_eq!(&r[..6], &[0, 0x0C, 0, 1, 0, 180]);
        assert_eq!(brightness(250)[5], 200);
    }

    #[test]
    fn effect_layout() {
        let s = EffectSettings {
            effect: Effect::Breathing,
            speed: 2,
            direction: Direction::LeftToRight,
            colors: vec![Rgb::new(1, 2, 3), Rgb::new(4, 5, 6)],
            theme: Theme::Ocean,
            brightness: 50,
        };
        let r = effect(&s);
        assert_eq!(&r[..5], &[0, 0x03, 0, 36, 0]);
        assert_eq!(r[5], 2); // breathing
        assert_eq!(r[6], 1); // multi color
        assert_eq!(r[7], 1); // two colors
        assert_eq!(r[9], 0);
        assert_eq!(&r[29..35], &[1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn ack_classification() {
        let mut resp = [0u8; 65];
        resp[5] = 0xEC;
        resp[6] = 0xAC;
        assert_eq!(classify(&resp), Ack::Accepted);
        resp[6] = 0xFA;
        assert_eq!(classify(&resp), Ack::Rejected);
    }

    #[test]
    fn color_parsing() {
        assert_eq!(Rgb::parse("rouge"), Some(Rgb::new(255, 0, 0)));
        assert_eq!(Rgb::parse("#00FF80"), Some(Rgb::new(0, 255, 0x80)));
        assert_eq!(Rgb::parse("nope"), None);
    }
}
