//! System sensors that need no vendor driver:
//! - CPU load, GPU engine load and ACPI thermal zones through PDH
//!   (the counters behind Task Manager);
//! - NVIDIA GPU temperature/power through NVML (`nvml.dll`, shipped with the
//!   NVIDIA driver), loaded dynamically;
//! - AC/battery state.

use std::ffi::c_void;

use windows::core::{s, PCWSTR};
use windows::Win32::Foundation::HMODULE;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhGetFormattedCounterValue, PdhOpenQueryW, PDH_FMT_COUNTERVALUE, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_HCOUNTER, PDH_HQUERY, PDH_MORE_DATA,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// PDH counters sampled together.
pub struct PdhSensors {
    query: PDH_HQUERY,
    cpu: Option<PDH_HCOUNTER>,
    gpu: Option<PDH_HCOUNTER>,
    zones: Option<PDH_HCOUNTER>,
}

// PDH handles may be used from any thread (one owner at a time).
unsafe impl Send for PdhSensors {}

#[derive(Debug, Clone, Default)]
pub struct PdhSample {
    pub cpu_load: Option<f32>,
    /// Highest load of a GPU (sum of its 3D engines), all adapters.
    pub gpu_load: Option<f32>,
    /// Hottest ACPI thermal zone, °C.
    pub thermal_zone: Option<f32>,
}

impl PdhSensors {
    pub fn new() -> Option<Self> {
        unsafe {
            let mut query = PDH_HQUERY::default();
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                return None;
            }
            let add = |path: &str| -> Option<PDH_HCOUNTER> {
                let w = wide(path);
                let mut c = PDH_HCOUNTER::default();
                (PdhAddEnglishCounterW(query, PCWSTR(w.as_ptr()), 0, &mut c) == 0).then_some(c)
            };
            let s = Self {
                query,
                cpu: add("\\Processor(_Total)\\% Processor Time"),
                gpu: add("\\GPU Engine(*engtype_3D)\\Utilization Percentage"),
                zones: add("\\Thermal Zone Information(*)\\High Precision Temperature"),
            };
            // Rate counters need a first collection as a baseline.
            PdhCollectQueryData(query);
            Some(s)
        }
    }

    fn array(counter: PDH_HCOUNTER) -> Vec<(String, f64)> {
        unsafe {
            let (mut size, mut count) = (0u32, 0u32);
            let r = PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, None);
            if r != PDH_MORE_DATA || size == 0 {
                return Vec::new();
            }
            // Buffer holds the items followed by their name strings.
            let item = std::mem::size_of::<PDH_FMT_COUNTERVALUE_ITEM_W>();
            let mut buf = vec![0u8; size as usize + item];
            let items = buf.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W;
            if PdhGetFormattedCounterArrayW(counter, PDH_FMT_DOUBLE, &mut size, &mut count, Some(items)) != 0 {
                return Vec::new();
            }
            (0..count as usize)
                .filter_map(|i| {
                    let it = &*items.add(i);
                    if it.FmtValue.CStatus != 0 {
                        return None;
                    }
                    let name = it.szName.to_string().unwrap_or_default();
                    Some((name, it.FmtValue.Anonymous.doubleValue))
                })
                .collect()
        }
    }

    pub fn sample(&mut self) -> PdhSample {
        let mut out = PdhSample::default();
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return out;
            }
            if let Some(c) = self.cpu {
                let mut v = PDH_FMT_COUNTERVALUE::default();
                if PdhGetFormattedCounterValue(c, PDH_FMT_DOUBLE, None, &mut v) == 0 && v.CStatus == 0 {
                    out.cpu_load = Some(v.Anonymous.doubleValue.clamp(0.0, 100.0) as f32);
                }
            }
        }
        if let Some(c) = self.gpu {
            // Instance names look like "pid_123_luid_0x..._0x..._phys_0_eng_0_engtype_3D":
            // sum per adapter (luid), keep the busiest adapter.
            let mut per_adapter: Vec<(String, f64)> = Vec::new();
            for (name, v) in Self::array(c) {
                let luid = name
                    .split("_phys")
                    .next()
                    .and_then(|s| s.split("luid_").nth(1))
                    .unwrap_or("")
                    .to_string();
                match per_adapter.iter_mut().find(|(l, _)| *l == luid) {
                    Some((_, sum)) => *sum += v,
                    None => per_adapter.push((luid, v)),
                }
            }
            if !per_adapter.is_empty() {
                let max = per_adapter.iter().map(|(_, v)| *v).fold(0.0, f64::max);
                out.gpu_load = Some(max.clamp(0.0, 100.0) as f32);
            }
        }
        if let Some(c) = self.zones {
            // Tenths of kelvin.
            out.thermal_zone = Self::array(c)
                .into_iter()
                .map(|(_, v)| (v / 10.0 - 273.15) as f32)
                .filter(|t| *t > 0.0 && *t < 130.0)
                .fold(None, |acc: Option<f32>, t| Some(acc.map_or(t, |a| a.max(t))));
        }
        out
    }
}

impl Drop for PdhSensors {
    fn drop(&mut self) {
        unsafe {
            PdhCloseQuery(self.query);
        }
    }
}

/// NVIDIA GPU sensors through NVML.
pub struct Nvml {
    _lib: HMODULE,
    device: *mut c_void,
    get_temp: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32,
    get_power: unsafe extern "C" fn(*mut c_void, *mut u32) -> i32,
    get_util: unsafe extern "C" fn(*mut c_void, *mut [u32; 2]) -> i32,
    pub name: String,
}

unsafe impl Send for Nvml {}

#[derive(Debug, Clone, Copy, Default)]
pub struct GpuSample {
    pub temp: Option<f32>,
    pub power_w: Option<f32>,
    pub load: Option<f32>,
}

impl Nvml {
    pub fn load() -> Option<Self> {
        unsafe {
            let path = wide("nvml.dll");
            let lib = LoadLibraryW(PCWSTR(path.as_ptr())).ok()?;
            let init: unsafe extern "C" fn() -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlInit_v2"))?);
            let by_index: unsafe extern "C" fn(u32, *mut *mut c_void) -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetHandleByIndex_v2"))?);
            let get_name: unsafe extern "C" fn(*mut c_void, *mut u8, u32) -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetName"))?);
            let get_temp: unsafe extern "C" fn(*mut c_void, u32, *mut u32) -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetTemperature"))?);
            let get_power: unsafe extern "C" fn(*mut c_void, *mut u32) -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetPowerUsage"))?);
            let get_util: unsafe extern "C" fn(*mut c_void, *mut [u32; 2]) -> i32 =
                std::mem::transmute(GetProcAddress(lib, s!("nvmlDeviceGetUtilizationRates"))?);
            if init() != 0 {
                return None;
            }
            let mut device = std::ptr::null_mut();
            if by_index(0, &mut device) != 0 {
                return None;
            }
            let mut name = [0u8; 96];
            let name = if get_name(device, name.as_mut_ptr(), name.len() as u32) == 0 {
                let end = name.iter().position(|b| *b == 0).unwrap_or(name.len());
                String::from_utf8_lossy(&name[..end]).into_owned()
            } else {
                "GPU NVIDIA".into()
            };
            Some(Self {
                _lib: lib,
                device,
                get_temp,
                get_power,
                get_util,
                name,
            })
        }
    }

    pub fn sample(&self) -> GpuSample {
        unsafe {
            let mut t = 0u32;
            let mut p = 0u32;
            let mut u = [0u32; 2];
            GpuSample {
                temp: ((self.get_temp)(self.device, 0, &mut t) == 0).then_some(t as f32),
                power_w: ((self.get_power)(self.device, &mut p) == 0).then_some(p as f32 / 1000.0),
                load: ((self.get_util)(self.device, &mut u) == 0).then_some(u[0] as f32),
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PowerState {
    pub on_ac: bool,
    /// Battery charge, if a battery is present.
    pub battery_percent: Option<u8>,
}

pub fn power_state() -> PowerState {
    let mut s = SYSTEM_POWER_STATUS::default();
    unsafe {
        if GetSystemPowerStatus(&mut s).is_err() {
            return PowerState { on_ac: true, battery_percent: None };
        }
    }
    PowerState {
        // 0 = offline, 1 = online, 255 = unknown (treat as AC).
        on_ac: s.ACLineStatus != 0,
        battery_percent: (s.BatteryLifePercent <= 100 && s.BatteryFlag & 128 == 0)
            .then_some(s.BatteryLifePercent),
    }
}
