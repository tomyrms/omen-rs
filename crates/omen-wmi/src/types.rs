//! Decoded structures for BIOS responses and parameters.

use std::fmt;
use std::str::FromStr;

/// System design data (`0x28`).
#[derive(Debug, Clone, Copy)]
pub struct SystemData {
    pub status_flags: u16,
    pub unknown2: u8,
    pub thermal_policy: u8,
    pub support_flags: u8,
    pub default_cpu_pl4: u8,
    pub bios_oc: u8,
    pub gpu_mode_switch: u8,
    pub default_cpu_power_with_gpu: u8,
}

impl SystemData {
    pub fn parse(data: &[u8]) -> Self {
        let get = |i: usize| data.get(i).copied().unwrap_or(0);
        Self {
            status_flags: (get(0) as u16) | ((get(1) as u16) << 8),
            unknown2: get(2),
            thermal_policy: get(3),
            support_flags: get(4),
            default_cpu_pl4: get(5),
            bios_oc: get(6),
            gpu_mode_switch: get(7),
            default_cpu_power_with_gpu: get(8),
        }
    }

    /// Bit 0 of the support flags: software fan control supported.
    pub fn sw_fan_control(&self) -> bool {
        self.support_flags & 0x01 != 0
    }

    pub fn extreme_mode(&self) -> bool {
        self.support_flags & 0x02 != 0
    }
}

/// Current fan speed levels (`0x2D`), raw units observed from 0x15 to 0x39.
#[derive(Debug, Clone, Copy)]
pub struct FanLevels {
    pub cpu: u8,
    pub gpu: u8,
}

impl FanLevels {
    /// Convert a raw level to an approximate percentage of the observed range.
    pub fn cpu_percent(&self) -> u32 {
        level_to_percent(self.cpu)
    }

    pub fn gpu_percent(&self) -> u32 {
        level_to_percent(self.gpu)
    }
}

fn level_to_percent(level: u8) -> u32 {
    // Observed levels: 0x15 (21) .. 0x39 (57) as the useful range.
    const MIN: f32 = 0x15 as f32;
    const MAX: f32 = 0x39 as f32;
    let clamped = (level as f32).clamp(MIN, MAX);
    (((clamped - MIN) / (MAX - MIN)) * 100.0) as u32
}

/// GPU power state (`0x21` read / `0x22` write).
#[derive(Debug, Clone, Copy, Default)]
pub struct GpuPowerData {
    /// 0 = base TGP only, 1 = custom TGP enabled.
    pub custom_tgp: u8,
    /// 0 = PPAB disabled, 1 = PPAB enabled.
    pub ppab: u8,
    /// GPU device power state (observed 1).
    pub d_state: u8,
    /// Peak temperature threshold in degrees Celsius.
    pub peak_temperature: u8,
}

impl GpuPowerData {
    pub fn parse(data: &[u8]) -> Self {
        let get = |i: usize| data.get(i).copied().unwrap_or(0);
        Self {
            custom_tgp: get(0),
            ppab: get(1),
            d_state: get(2),
            peak_temperature: get(3),
        }
    }

    pub fn to_bytes(self) -> [u8; 4] {
        [self.custom_tgp, self.ppab, self.d_state, self.peak_temperature]
    }
}

/// CPU power limits (`0x29` write). `0xFF` means "no change".
#[derive(Debug, Clone, Copy)]
pub struct CpuPowerData {
    pub limit1: u8,
    pub limit2: u8,
    pub limit4: u8,
    pub limit_with_gpu: u8,
}

impl Default for CpuPowerData {
    fn default() -> Self {
        Self {
            limit1: 0xFF,
            limit2: 0xFF,
            limit4: 0xFF,
            limit_with_gpu: 0xFF,
        }
    }
}

impl CpuPowerData {
    pub fn to_bytes(self) -> [u8; 4] {
        [self.limit1, self.limit2, self.limit4, self.limit_with_gpu]
    }
}

/// Thermal throttling state (`0x35`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Throttling {
    Unknown,
    On,
    Default,
    Other(u8),
}

impl Throttling {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => Throttling::Unknown,
            0x01 => Throttling::On,
            0x04 => Throttling::Default,
            other => Throttling::Other(other),
        }
    }
}

/// Fan performance mode (`0x1A` write). Values match the enum used by the
/// vendor application (`HP.Omen.Core.Common.PowerControl.PerformanceMode`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FanMode {
    LegacyDefault = 0x00,
    LegacyPerformance = 0x01,
    LegacyCool = 0x02,
    LegacyQuiet = 0x03,
    LegacyExtreme = 0x04,
    L0 = 0x10,
    L5 = 0x11,
    L1 = 0x20,
    L6 = 0x21,
    Default = 0x30,
    Performance = 0x31,
    L3 = 0x40,
    Cool = 0x50,
}

impl FromStr for FanMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "legacy-default" => Ok(FanMode::LegacyDefault),
            "legacy-performance" => Ok(FanMode::LegacyPerformance),
            "legacy-cool" => Ok(FanMode::LegacyCool),
            "legacy-quiet" => Ok(FanMode::LegacyQuiet),
            "legacy-extreme" => Ok(FanMode::LegacyExtreme),
            "l0" => Ok(FanMode::L0),
            "l1" => Ok(FanMode::L1),
            "l3" => Ok(FanMode::L3),
            "l5" => Ok(FanMode::L5),
            "l6" => Ok(FanMode::L6),
            "default" => Ok(FanMode::Default),
            "performance" => Ok(FanMode::Performance),
            "cool" => Ok(FanMode::Cool),
            _ => Err(format!("unknown fan mode: {s}")),
        }
    }
}

impl fmt::Display for FanMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

/// One entry of the BIOS dynamic fan table (`0x2F` read / `0x32` write).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FanTableEntry {
    pub cpu: u8,
    pub gpu: u8,
    pub temp_c: u8,
}

/// BIOS dynamic fan table (`0x2F`). Note: writing (`0x32`) is documented as
/// desktop-only; on laptops the call succeeds but takes no effect.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FanTable {
    pub fan_count: u8,
    pub level_count: u8,
    pub entries: Vec<FanTableEntry>,
}

impl FanTable {
    pub fn parse(data: &[u8]) -> Self {
        let fan_count = data.first().copied().unwrap_or(0);
        let level_count = data.get(1).copied().unwrap_or(0).min(14);
        let mut entries = Vec::with_capacity(level_count as usize);
        for i in 0..level_count as usize {
            let base = 2 + 3 * i;
            entries.push(FanTableEntry {
                cpu: data.get(base).copied().unwrap_or(0),
                gpu: data.get(base + 1).copied().unwrap_or(0),
                temp_c: data.get(base + 2).copied().unwrap_or(0),
            });
        }
        Self {
            fan_count,
            level_count,
            entries,
        }
    }

    pub fn to_bytes(&self) -> [u8; 128] {
        let mut buf = [0u8; 128];
        buf[0] = self.fan_count;
        buf[1] = self.entries.len() as u8;
        for (i, e) in self.entries.iter().take(14).enumerate() {
            buf[2 + 3 * i] = e.cpu;
            buf[3 + 3 * i] = e.gpu;
            buf[4 + 3 * i] = e.temp_c;
        }
        buf
    }
}

/// Keyboard backlight switch (`0x04` read / `0x05` write, family `0x20009`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backlight {
    Off,
    On,
}

impl Backlight {
    pub fn from_byte(b: u8) -> Self {
        if b == 0xE4 {
            Backlight::On
        } else {
            Backlight::Off
        }
    }

    pub fn to_byte(self) -> u8 {
        match self {
            Backlight::Off => 0x64,
            Backlight::On => 0xE4,
        }
    }
}

/// One RGB zone of the keyboard backlight color table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorZone {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

/// Keyboard backlight color table (`0x02` read / `0x03` write, family `0x20009`).
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ColorTable {
    pub zone_count: u8,
    pub zones: Vec<ColorZone>,
}

impl ColorTable {
    /// Size of the padding between the zone count and the first color entry.
    const PAD: usize = 24;

    pub fn parse(data: &[u8]) -> Self {
        let zone_count = data.first().copied().unwrap_or(0).min(3);
        let mut zones = Vec::with_capacity(zone_count as usize + 1);
        for i in 0..=zone_count as usize {
            let base = 1 + Self::PAD + 3 * i;
            zones.push(ColorZone {
                r: data.get(base).copied().unwrap_or(0),
                g: data.get(base + 1).copied().unwrap_or(0),
                b: data.get(base + 2).copied().unwrap_or(0),
            });
        }
        Self { zone_count, zones }
    }

    pub fn to_bytes(&self) -> [u8; 128] {
        let mut buf = [0u8; 128];
        buf[0] = self.zone_count.min(3);
        for (i, z) in self.zones.iter().take(4).enumerate() {
            let base = 1 + Self::PAD + 3 * i;
            buf[base] = z.r;
            buf[base + 1] = z.g;
            buf[base + 2] = z.b;
        }
        buf
    }
}

/// Keyboard type as reported by the BIOS (`0x2B`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KbdType {
    Standard,
    WithNumPad,
    TenKeyLess,
    PerKeyRgb,
    Unknown(u8),
}

impl KbdType {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => KbdType::Standard,
            0x01 => KbdType::WithNumPad,
            0x02 => KbdType::TenKeyLess,
            0x03 => KbdType::PerKeyRgb,
            other => KbdType::Unknown(other),
        }
    }
}

/// Graphics mode (legacy MUX switch, `0x52` read family `0x00001`,
/// write family `0x00002`; a reboot is required for a change to apply).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GpuMode {
    Hybrid,
    Discrete,
    Optimus,
    Unknown(u8),
}

impl GpuMode {
    pub fn from_byte(b: u8) -> Self {
        match b {
            0x00 => GpuMode::Hybrid,
            0x01 => GpuMode::Discrete,
            0x02 => GpuMode::Optimus,
            other => GpuMode::Unknown(other),
        }
    }

    pub fn to_byte(self) -> Option<u8> {
        match self {
            GpuMode::Hybrid => Some(0x00),
            GpuMode::Discrete => Some(0x01),
            GpuMode::Optimus => Some(0x02),
            GpuMode::Unknown(_) => None,
        }
    }
}

/// One point of a software fan curve (temperature -> fan levels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FanCurvePoint {
    pub temp_c: u8,
    pub cpu_level: u8,
    pub gpu_level: u8,
}

/// Software fan curve: maps the thermal sensor reading to fan speed levels and
/// is applied periodically through `SetFanLevel` (`0x2E`). Levels are raw
/// units observed from 0x14 (~2000 rpm) to 0x3B; values are clamped.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FanCurve {
    pub points: Vec<FanCurvePoint>,
}

impl Default for FanCurve {
    fn default() -> Self {
        Self::default_curve()
    }
}

impl FanCurve {
    pub const LEVEL_MIN: u8 = 0x14;
    pub const LEVEL_MAX: u8 = 0x3B;

    /// A sane default curve for desk use.
    pub fn default_curve() -> Self {
        Self {
            points: vec![
                FanCurvePoint {
                    temp_c: 40,
                    cpu_level: 0x1A,
                    gpu_level: 0x1A,
                },
                FanCurvePoint {
                    temp_c: 55,
                    cpu_level: 0x22,
                    gpu_level: 0x22,
                },
                FanCurvePoint {
                    temp_c: 68,
                    cpu_level: 0x2C,
                    gpu_level: 0x2C,
                },
                FanCurvePoint {
                    temp_c: 78,
                    cpu_level: 0x36,
                    gpu_level: 0x36,
                },
                FanCurvePoint {
                    temp_c: 88,
                    cpu_level: Self::LEVEL_MAX,
                    gpu_level: Self::LEVEL_MAX,
                },
            ],
        }
    }

    /// Interpolates the fan levels for a temperature. Points must be sorted
    /// by temperature; below the first point the first value is used, above
    /// the last point the last value is used.
    pub fn interpolate(&self, temp_c: u8) -> (u8, u8) {
        if self.points.is_empty() {
            return (FanCurve::LEVEL_MIN, FanCurve::LEVEL_MIN);
        }
        let clamped_temp = temp_c;
        let first = self.points[0];
        if clamped_temp <= first.temp_c {
            return (Self::clamp(first.cpu_level), Self::clamp(first.gpu_level));
        }
        for w in self.points.windows(2) {
            let (a, b) = (w[0], w[1]);
            if clamped_temp <= b.temp_c {
                let span = b.temp_c.saturating_sub(a.temp_c).max(1) as f32;
                let t = (clamped_temp.saturating_sub(a.temp_c)) as f32 / span;
                let lerp = |x: u8, y: u8| -> u8 {
                    (x as f32 + (y as f32 - x as f32) * t).round() as u8
                };
                return (
                    Self::clamp(lerp(a.cpu_level, b.cpu_level)),
                    Self::clamp(lerp(a.gpu_level, b.gpu_level)),
                );
            }
        }
        let last = *self.points.last().unwrap();
        (Self::clamp(last.cpu_level), Self::clamp(last.gpu_level))
    }

    fn clamp(level: u8) -> u8 {
        level.clamp(Self::LEVEL_MIN, Self::LEVEL_MAX)
    }
}

/// A snapshot of the machine state sampled by the monitoring loop.
#[derive(Debug, Clone, Default)]
pub struct SensorSnapshot {
    pub temp_c: Option<u8>,
    pub fan_cpu: Option<u8>,
    pub fan_gpu: Option<u8>,
    pub fan_cpu_percent: Option<u32>,
    pub fan_gpu_percent: Option<u32>,
    pub max_fan: Option<bool>,
    pub throttling: Option<String>,
    pub power_raw: Vec<u8>,
    pub system: Option<SystemData>,
    pub gpu_power: Option<GpuPowerData>,
    pub error: Option<String>,
}
