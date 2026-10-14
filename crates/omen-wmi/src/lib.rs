//! Client for the HP BIOS WMI interface (`root\WMI`, `hpqBIntM` / `hpqBIOSInt<n>`).
//!
//! Protocol facts were established from:
//! - REA static analysis of the OMEN Gaming Hub .NET assemblies on the target
//!   machine (command IDs called through `BiosWmiCmd_Get/Set(0x20008, ...)`),
//! - the public protocol notes of the OmenMon project (GPL-3.0), used strictly
//!   as factual interface documentation (parameter classes, the `SECU` sign,
//!   fan/power command identifiers),
//! - live read-only verification against the target machine.
//!
//! This implementation is original and does not copy OmenMon code.
//!
//! The interface requires an elevated token (administrator), like the vendor
//! application and OmenMon.

mod commands;
mod types;

pub use commands::*;
pub use types::*;

use std::mem::ManuallyDrop;
use std::ptr;

use windows::core::{Error, IUnknown, Interface, Result as WinResult, BSTR};
use windows::Win32::Foundation::E_FAIL;
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoInitializeSecurity, CLSCTX_INPROC_SERVER,
    COINIT_MULTITHREADED, EOAC_NONE, RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
};
use windows::Win32::System::Ole::{
    SafeArrayAccessData, SafeArrayCreateVector, SafeArrayGetLBound, SafeArrayGetUBound,
    SafeArrayUnaccessData,
};
use windows::Win32::System::Variant::{
    VARIANT, VARIANT_0, VARIANT_0_0, VARIANT_0_0_0, VARENUM, VT_ARRAY, VT_I4, VT_NULL, VT_UI1,
    VT_UI4, VT_UNKNOWN,
};
use windows::Win32::System::Wmi::{
    IWbemClassObject, IWbemLocator, IWbemServices, WBEM_GENERIC_FLAG_TYPE, WbemLocator,
};

const WMI_NAMESPACE: &str = "ROOT\\WMI";
const METHOD_CLASS: &str = "hpqBIntM";
const INSTANCE_PATH: &str = "hpqBIntM.InstanceName=\"ACPI\\\\PNP0C14\\\\0_0\"";
const INSTANCE_PATH_ALT: &str = "hpqBIntM.InstanceName=\"ACPI\\\\PNP0C14\\\\1_0\"";
const DATA_CLASS: &str = "hpqBDataIn";
const DATA_FIELD: &str = "hpqBData";
/// Authorization secret expected by the HP BIOS WMI interface ("SECU").
const SIGN: [u8; 4] = [0x53, 0x45, 0x43, 0x55];

/// CIM type identifiers used in `IWbemClassObject::Put`.
///
/// Note: for `uint32` properties the VARIANT must carry a *signed* `VT_I4`
/// value while the declared type stays `CIM_UINT32` (19); passing a `VT_UI4`
/// VARIANT is rejected with `WBEM_E_TYPE_MISMATCH` by this provider.
const CIM_TYPE_UI4: i32 = VT_UI4.0 as i32;
const CIM_TYPE_UNKNOWN: i32 = VT_UNKNOWN.0 as i32;
const CIM_TYPE_UI1_ARRAY: i32 = (VT_ARRAY.0 | VT_UI1.0) as i32;

fn fail(msg: &str) -> Error {
    Error::new(E_FAIL, msg)
}

fn hr(e: &Error) -> String {
    format!("0x{:08X} {}", e.code().0 as u32, e.message())
}

fn status(res: &WinResult<()>) -> String {
    match res {
        Ok(()) => "ok".to_string(),
        Err(e) => hr(e),
    }
}

fn hres(h: windows::core::HRESULT) -> String {
    if h.is_ok() {
        "ok".to_string()
    } else {
        hr(&Error::from_hresult(h))
    }
}

fn step(name: &str, e: Error) -> Error {
    Error::new(e.code(), format!("{name}: {}", e.message()))
}

/// Byte-array VARIANT backed by a newly allocated SAFEARRAY.
///
/// The SAFEARRAY is intentionally not freed: WMI may keep a reference to the
/// property while the method call is in flight, and these are short-lived CLI
/// processes. Long-running consumers should revisit this ownership decision.
fn variant_from_bytes(bytes: &[u8]) -> WinResult<VARIANT> {
    unsafe {
        let sa = SafeArrayCreateVector(VT_UI1, 0, bytes.len() as u32);
        if sa.is_null() {
            return Err(Error::from_thread());
        }
        let mut data: *mut core::ffi::c_void = ptr::null_mut();
        SafeArrayAccessData(sa, &mut data)?;
        if !bytes.is_empty() {
            ptr::copy_nonoverlapping(bytes.as_ptr(), data.cast::<u8>(), bytes.len());
        }
        let _ = SafeArrayUnaccessData(sa);

        Ok(VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VARENUM(VT_ARRAY.0 | VT_UI1.0),
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: VARIANT_0_0_0 { parray: sa },
                }),
            },
        })
    }
}

fn variant_from_i32(value: i32) -> VARIANT {
    VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VT_I4,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: VARIANT_0_0_0 { lVal: value },
                }),
            },
        }
}

fn variant_from_u32(value: u32) -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_UI4,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { ulVal: value },
            }),
        },
    }
}

fn variant_null() -> VARIANT {
    VARIANT {
        Anonymous: VARIANT_0 {
            Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                vt: VT_NULL,
                wReserved1: 0,
                wReserved2: 0,
                wReserved3: 0,
                Anonymous: VARIANT_0_0_0 { lVal: 0 },
            }),
        },
    }
}

fn variant_from_unknown(unk: &IUnknown) -> VARIANT {
    VARIANT {
            Anonymous: VARIANT_0 {
                Anonymous: ManuallyDrop::new(VARIANT_0_0 {
                    vt: VT_UNKNOWN,
                    wReserved1: 0,
                    wReserved2: 0,
                    wReserved3: 0,
                    Anonymous: VARIANT_0_0_0 {
                        punkVal: ManuallyDrop::new(Some(unk.clone())),
                    },
                }),
            },
        }
}

fn variant_as_i32(v: &VARIANT) -> Option<i32> {
    unsafe {
        match v.Anonymous.Anonymous.vt {
            VT_I4 => Some(v.Anonymous.Anonymous.Anonymous.lVal),
            VT_UI4 => Some(v.Anonymous.Anonymous.Anonymous.ulVal as i32),
            _ => None,
        }
    }
}

fn variant_as_unknown(v: &VARIANT) -> Option<IUnknown> {
    unsafe {
        if v.Anonymous.Anonymous.vt == VT_UNKNOWN {
            (*v.Anonymous.Anonymous.Anonymous.punkVal).clone()
        } else {
            None
        }
    }
}

fn variant_as_bytes(v: &VARIANT) -> Vec<u8> {
    unsafe {
        let vt = v.Anonymous.Anonymous.vt.0 as u32;
        if vt & VT_ARRAY.0 as u32 == 0 || vt & VT_UI1.0 as u32 == 0 {
            return Vec::new();
        }
        let sa = v.Anonymous.Anonymous.Anonymous.parray;
        if sa.is_null() {
            return Vec::new();
        }
        let lbound = match SafeArrayGetLBound(sa, 1) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let ubound = match SafeArrayGetUBound(sa, 1) {
            Ok(v) => v,
            Err(_) => return Vec::new(),
        };
        let len = (ubound - lbound + 1).max(0) as usize;
        let mut data: *mut core::ffi::c_void = ptr::null_mut();
        if SafeArrayAccessData(sa, &mut data).is_err() {
            return Vec::new();
        }
        let out = core::slice::from_raw_parts(data as *const u8, len).to_vec();
        let _ = SafeArrayUnaccessData(sa);
        out
    }
}

/// Connected client for the HP BIOS WMI interface.
pub struct BiosClient {
    services: IWbemServices,
    method_class: IWbemClassObject,
    data_class: IWbemClassObject,
}

impl BiosClient {
    /// Connects to `root\WMI` and prepares the `hpqBIntM` method/data classes.
    /// Requires an elevated process token on OMEN hardware.
    pub fn connect() -> WinResult<Self> {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
            // Default security; RPC_E_TOO_LATE is fine when already initialized.
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

            let locator: IWbemLocator = CoCreateInstance(&WbemLocator, None, CLSCTX_INPROC_SERVER)
                .map_err(|e| step("CoCreateInstance", e))?;

            let services: IWbemServices = locator
                .ConnectServer(
                    &BSTR::from(WMI_NAMESPACE),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                )
                .map_err(|e| step("ConnectServer", e))?;

            let mut method_class: Option<IWbemClassObject> = None;
            services
                .GetObject(
                    &BSTR::from(METHOD_CLASS),
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&mut method_class),
                    None,
                )
                .map_err(|e| step("GetObject(hpqBIntM)", e))?;
            let method_class =
                method_class.ok_or_else(|| fail("hpqBIntM class not available"))?;

            let mut data_class: Option<IWbemClassObject> = None;
            services
                .GetObject(
                    &BSTR::from(DATA_CLASS),
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    Some(&mut data_class),
                    None,
                )
                .map_err(|e| step("GetObject(hpqBDataIn)", e))?;
            let data_class = data_class.ok_or_else(|| fail("hpqBDataIn class not available"))?;

            Ok(Self {
                services,
                method_class,
                data_class,
            })
        }
    }

    /// Low-level call: sends one BIOS command and returns `(rwReturnCode, out bytes)`.
    ///
    /// `out_size` must be one of 0, 4, 128, 1024, 4096 and selects the
    /// corresponding `hpqBIOSInt<n>` method.
    pub fn send(
        &self,
        command: u32,
        command_type: u32,
        payload: Option<&[u8]>,
        out_size: u32,
    ) -> WinResult<(i32, Vec<u8>)> {
        let method_name = match out_size {
            0 => "hpqBIOSInt0",
            4 => "hpqBIOSInt4",
            128 => "hpqBIOSInt128",
            1024 => "hpqBIOSInt1024",
            4096 => "hpqBIOSInt4096",
            _ => return Err(fail("out_size must be 0, 4, 128, 1024 or 4096")),
        };

        unsafe {
            let mut method: Option<IWbemClassObject> = None;
            self.method_class
                .GetMethod(&BSTR::from(method_name), 0, &mut method, ptr::null_mut())
                .map_err(|e| step(&format!("GetMethod({method_name})"), e))?;
            let method = method.ok_or_else(|| fail("BIOS method not found"))?;

            let in_params = method
                .SpawnInstance(0)
                .map_err(|e| step("SpawnInstance(method params)", e))?;

            let data_inst = self
                .data_class
                .SpawnInstance(0)
                .map_err(|e| step("SpawnInstance(hpqBDataIn)", e))?;

            let sign_v = variant_from_bytes(&SIGN)?;
            data_inst
                .Put(&BSTR::from("Sign"), 0, &sign_v, CIM_TYPE_UI1_ARRAY)
                .map_err(|e| step("Put(Sign)", e))?;

            let command_v = variant_from_i32(command as i32);
            data_inst
                .Put(&BSTR::from("Command"), 0, &command_v, CIM_TYPE_UI4)
                .map_err(|e| step("Put(Command)", e))?;

            let command_type_v = variant_from_i32(command_type as i32);
            data_inst
                .Put(
                    &BSTR::from("CommandType"),
                    0,
                    &command_type_v,
                    CIM_TYPE_UI4,
                )
                .map_err(|e| step("Put(CommandType)", e))?;

            match payload {
                Some(bytes) => {
                    let data_v = variant_from_bytes(bytes)?;
                    data_inst
                        .Put(&BSTR::from(DATA_FIELD), 0, &data_v, CIM_TYPE_UI1_ARRAY)
                        .map_err(|e| step("Put(hpqBData)", e))?;
                    let size_v = variant_from_i32(bytes.len() as i32);
                    data_inst
                        .Put(&BSTR::from("Size"), 0, &size_v, CIM_TYPE_UI4)
                        .map_err(|e| step("Put(Size)", e))?;
                }
                None => {
                    let size_v = variant_from_i32(0);
                    data_inst
                        .Put(&BSTR::from("Size"), 0, &size_v, CIM_TYPE_UI4)
                        .map_err(|e| step("Put(Size)", e))?;
                }
            }

            let data_unknown: IUnknown = data_inst.into();
            let in_data_v = variant_from_unknown(&data_unknown);
            in_params
                .Put(&BSTR::from("InData"), 0, &in_data_v, CIM_TYPE_UNKNOWN)
                .map_err(|e| step("Put(InData)", e))?;

            let mut out_params: Option<IWbemClassObject> = None;
            self.services
                .ExecMethod(
                    &BSTR::from(INSTANCE_PATH),
                    &BSTR::from(method_name),
                    WBEM_GENERIC_FLAG_TYPE(0),
                    None,
                    &in_params,
                    Some(&mut out_params),
                    None,
                )
                .map_err(|e| step("ExecMethod", e))?;
            let out_params = out_params.ok_or_else(|| fail("no method output"))?;

            let mut out_v = VARIANT::default();
            out_params
                .Get(&BSTR::from("OutData"), 0, &mut out_v, None, None)
                .map_err(|e| step("Get(OutData)", e))?;
            let out_obj: IWbemClassObject = variant_as_unknown(&out_v)
                .and_then(|u| u.cast().ok())
                .ok_or_else(|| fail("cannot convert OutData"))?;

            let mut code_v = VARIANT::default();
            out_obj
                .Get(&BSTR::from("rwReturnCode"), 0, &mut code_v, None, None)
                .map_err(|e| step("Get(rwReturnCode)", e))?;
            let code = variant_as_i32(&code_v).unwrap_or(-1);

            let mut data = Vec::new();
            if out_size > 0 {
                let mut data_v = VARIANT::default();
                if out_obj
                    .Get(&BSTR::from("Data"), 0, &mut data_v, None, None)
                    .is_ok()
                {
                    data = variant_as_bytes(&data_v);
                }
            }

            Ok((code, data))
        }
    }

    // -- Capability / system -------------------------------------------------

    pub fn get_system(&self) -> WinResult<(i32, SystemData)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_SYSTEM_DATA, None, 128)?;
        Ok((code, SystemData::parse(&data)))
    }

    pub fn get_fan_count(&self) -> WinResult<(i32, u8)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_FAN_COUNT, Some(&[0, 0, 0, 0]), 4)?;
        Ok((code, data.first().copied().unwrap_or(0)))
    }

    pub fn get_fan_type(&self) -> WinResult<(i32, u8)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_FAN_TYPE, Some(&[0, 0, 0, 0]), 128)?;
        Ok((code, data.first().copied().unwrap_or(0)))
    }

    // -- Thermal -------------------------------------------------------------

    pub fn get_fan_level(&self) -> WinResult<(i32, FanLevels)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_FAN_LEVEL, Some(&[0, 0, 0, 0]), 128)?;
        Ok((
            code,
            FanLevels {
                cpu: data.first().copied().unwrap_or(0),
                gpu: data.get(1).copied().unwrap_or(0),
            },
        ))
    }

    pub fn set_fan_level(&self, cpu: u8, gpu: u8) -> WinResult<i32> {
        let (code, _) =
            self.send(CMD_DEFAULT, cmd::SET_FAN_LEVEL, Some(&[cpu, gpu, 0, 0]), 0)?;
        Ok(code)
    }

    pub fn get_max_fan(&self) -> WinResult<(i32, bool)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_MAX_FAN, Some(&[0, 0, 0, 0]), 4)?;
        Ok((code, data.first().map(|b| b & 1 != 0).unwrap_or(false)))
    }

    pub fn set_max_fan(&self, on: bool) -> WinResult<i32> {
        let (code, _) = self.send(
            CMD_DEFAULT,
            cmd::SET_MAX_FAN,
            Some(&[u8::from(on), 0, 0, 0]),
            0,
        )?;
        Ok(code)
    }

    pub fn set_fan_mode(&self, mode: FanMode) -> WinResult<i32> {
        let (code, _) = self.send(
            CMD_DEFAULT,
            cmd::SET_FAN_MODE,
            Some(&[0xFF, mode as u8, 0, 0]),
            0,
        )?;
        Ok(code)
    }

    pub fn get_temperature(&self) -> WinResult<(i32, u8)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_TEMPERATURE, Some(&[1, 0, 0, 0]), 4)?;
        Ok((code, data.first().copied().unwrap_or(0)))
    }

    pub fn get_throttling(&self) -> WinResult<(i32, Throttling)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_THROTTLING, Some(&[0, 4, 0, 0]), 128)?;
        Ok((
            code,
            Throttling::from_byte(data.get(1).copied().unwrap_or(0)),
        ))
    }

    // -- Power ---------------------------------------------------------------

    pub fn get_gpu_power(&self) -> WinResult<(i32, GpuPowerData)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_GPU_POWER, Some(&[0, 0, 0, 0]), 4)?;
        Ok((code, GpuPowerData::parse(&data)))
    }

    pub fn set_gpu_power(&self, data: GpuPowerData) -> WinResult<i32> {
        let (code, _) = self.send(CMD_DEFAULT, cmd::SET_GPU_POWER, Some(&data.to_bytes()), 0)?;
        Ok(code)
    }

    pub fn set_cpu_power(&self, data: CpuPowerData) -> WinResult<i32> {
        let (code, _) = self.send(CMD_DEFAULT, cmd::SET_CPU_POWER, Some(&data.to_bytes()), 0)?;
        Ok(code)
    }

    /// Raw `0x2A` power data read (command observed in the OMEN Hub IL on
    /// 2023+ platforms; layout not fully decoded yet).
    pub fn get_power_data(&self) -> WinResult<(i32, Vec<u8>)> {
        self.send(CMD_DEFAULT, cmd::GET_POWER_DATA, Some(&[0, 0, 0, 0]), 128)
    }

    // -- BIOS fan table ------------------------------------------------------

    pub fn get_fan_table(&self) -> WinResult<(i32, FanTable)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_FAN_TABLE, Some(&[0, 0, 0, 0]), 128)?;
        Ok((code, FanTable::parse(&data)))
    }

    /// Desktop-only on most models: succeeds but has no effect on laptops.
    pub fn set_fan_table(&self, table: &FanTable) -> WinResult<i32> {
        let (code, _) = self.send(CMD_DEFAULT, cmd::SET_FAN_TABLE, Some(&table.to_bytes()), 0)?;
        Ok(code)
    }

    // -- Keyboard (family 0x20009) -------------------------------------------

    pub fn get_kbd_type(&self) -> WinResult<(i32, KbdType)> {
        let (code, data) = self.send(CMD_DEFAULT, cmd::GET_KBD_TYPE, Some(&[0, 0, 0, 0]), 4)?;
        Ok((code, KbdType::from_byte(data.first().copied().unwrap_or(0))))
    }

    pub fn has_backlight(&self) -> WinResult<(i32, bool)> {
        let (code, data) =
            self.send(CMD_KEYBOARD, cmd::GET_KBD_SUPPORT, Some(&[0, 0, 0, 0]), 4)?;
        Ok((code, data.first().map(|b| b & 1 != 0).unwrap_or(false)))
    }

    pub fn get_backlight(&self) -> WinResult<(i32, Backlight)> {
        let (code, data) = self.send(CMD_KEYBOARD, cmd::GET_BACKLIGHT, Some(&[0, 0, 0, 0]), 4)?;
        Ok((code, Backlight::from_byte(data.first().copied().unwrap_or(0))))
    }

    pub fn set_backlight(&self, value: Backlight) -> WinResult<i32> {
        let (code, _) = self.send(
            CMD_KEYBOARD,
            cmd::SET_BACKLIGHT,
            Some(&[value.to_byte(), 0, 0, 0]),
            0,
        )?;
        Ok(code)
    }

    pub fn get_color_table(&self) -> WinResult<(i32, ColorTable)> {
        let (code, data) =
            self.send(CMD_KEYBOARD, cmd::GET_COLOR_TABLE, Some(&[0, 0, 0, 0]), 128)?;
        Ok((code, ColorTable::parse(&data)))
    }

    pub fn set_color_table(&self, table: &ColorTable) -> WinResult<i32> {
        let (code, _) =
            self.send(CMD_KEYBOARD, cmd::SET_COLOR_TABLE, Some(&table.to_bytes()), 0)?;
        Ok(code)
    }

    // -- Graphics mode (MUX, reboot required to apply) -----------------------

    pub fn get_gpu_mode(&self) -> WinResult<(i32, GpuMode)> {
        let (code, data) = self.send(CMD_LEGACY, cmd::GET_GPU_MODE, None, 4)?;
        Ok((code, GpuMode::from_byte(data.first().copied().unwrap_or(0))))
    }

    /// Writes the graphics mode. A reboot is required for the change to take
    /// effect; the BIOS rejects unsupported modes with a non-zero code.
    pub fn set_gpu_mode(&self, mode: GpuMode) -> WinResult<i32> {
        let byte = mode
            .to_byte()
            .ok_or_else(|| fail("unsupported GPU mode value"))?;
        let (code, _) = self.send(CMD_GPU_MODE, cmd::SET_GPU_MODE, Some(&[byte, 0, 0, 0]), 0)?;
        Ok(code)
    }
}

/// Best-effort diagnostic: walks the connection steps and reports each one.
/// Run this elevated to diagnose `hpqBIntM` access problems.
pub fn connection_report() -> String {
    use std::fmt::Write as _;

    let mut r = String::new();
    unsafe {
        let co = CoInitializeEx(None, COINIT_MULTITHREADED);
        let _ = writeln!(r, "CoInitializeEx             : {}", hres(co));

        let sec = CoInitializeSecurity(
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
        let _ = writeln!(r, "CoInitializeSecurity       : {}", status(&sec));

        match CoCreateInstance::<_, IWbemLocator>(&WbemLocator, None, CLSCTX_INPROC_SERVER) {
            Ok(locator) => {
                let _ = writeln!(r, "CoCreateInstance(locator)  : ok");
                match locator.ConnectServer(
                    &BSTR::from(WMI_NAMESPACE),
                    &BSTR::new(),
                    &BSTR::new(),
                    &BSTR::new(),
                    0,
                    &BSTR::new(),
                    None,
                ) {
                    Ok(services) => {
                        let _ = writeln!(r, "ConnectServer({WMI_NAMESPACE}) : ok");
                        for class in [METHOD_CLASS, DATA_CLASS] {
                            let mut obj: Option<IWbemClassObject> = None;
                            let res = services.GetObject(
                                &BSTR::from(class),
                                WBEM_GENERIC_FLAG_TYPE(0),
                                None,
                                Some(&mut obj),
                                None,
                            );
                            let _ = writeln!(r, "GetObject(class {class}) : {}", status(&res));
                        }
                        for path in [INSTANCE_PATH, INSTANCE_PATH_ALT] {
                            let mut obj: Option<IWbemClassObject> = None;
                            let res = services.GetObject(
                                &BSTR::from(path),
                                WBEM_GENERIC_FLAG_TYPE(0),
                                None,
                                Some(&mut obj),
                                None,
                            );
                            let _ = writeln!(r, "GetObject({path}) : {}", status(&res));
                        }
                        // Declared CIM types of the hpqBDataIn class properties
                        let mut dump_cls: Option<IWbemClassObject> = None;
                        if services
                            .GetObject(
                                &BSTR::from(DATA_CLASS),
                                WBEM_GENERIC_FLAG_TYPE(0),
                                None,
                                Some(&mut dump_cls),
                                None,
                            )
                            .is_ok()
                        {
                            if let Some(dump_cls) = dump_cls {
                                for name in ["Sign", "Command", "CommandType", "Size", DATA_FIELD] {
                                    let mut v = VARIANT::default();
                                    let mut pt = 0i32;
                                    match dump_cls.Get(
                                        &BSTR::from(name),
                                        0,
                                        &mut v,
                                        Some(&mut pt),
                                        None,
                                    ) {
                                        Ok(()) => {
                                            let vt = v.Anonymous.Anonymous.vt.0;
                                            let _ = writeln!(
                                                r,
                                                "hpqBDataIn.{name:<11} : vt=0x{vt:04X} cim=0x{pt:04X}"
                                            );
                                        }
                                        Err(e) => {
                                            let _ = writeln!(r, "hpqBDataIn.{name} : {}", hr(&e));
                                        }
                                    }
                                }
                            }
                        }
                        let mut cls: Option<IWbemClassObject> = None;
                        if services
                            .GetObject(
                                &BSTR::from(METHOD_CLASS),
                                WBEM_GENERIC_FLAG_TYPE(0),
                                None,
                                Some(&mut cls),
                                None,
                            )
                            .is_ok()
                        {
                            if let Some(cls) = cls {
                                let mut m: Option<IWbemClassObject> = None;
                                let res = cls.GetMethod(
                                    &BSTR::from("hpqBIOSInt128"),
                                    0,
                                    &mut m,
                                    ptr::null_mut(),
                                );
                                let _ = writeln!(r, "GetMethod(hpqBIOSInt128)   : {}", status(&res));
                                if let Some(m) = m {
                                    let mut v = VARIANT::default();
                                    let mut pt = 0i32;
                                    match m.Get(&BSTR::from("InData"), 0, &mut v, Some(&mut pt), None) {
                                        Ok(()) => {
                                            let vt = v.Anonymous.Anonymous.vt.0;
                                            let _ = writeln!(
                                                r,
                                                "method.InData              : vt=0x{vt:04X} cim=0x{pt:04X}"
                                            );
                                        }
                                        Err(e) => {
                                            let _ = writeln!(r, "method.InData              : {}", hr(&e));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        let _ = writeln!(r, "ConnectServer              : {}", hr(&e));
                    }
                }
            }
            Err(e) => {
                let _ = writeln!(r, "CoCreateInstance           : {}", hr(&e));
            }
        }

        // Full send-path probe (types + method invocation).
        let _ = writeln!(r);
        match BiosClient::connect() {
            Ok(client) => match client.get_system() {
                Ok((code, _)) => {
                    let _ = writeln!(r, "send(get_system)           : ok (rwReturnCode {code})");
                }
                Err(e) => {
                    let _ = writeln!(r, "send(get_system)           : {}", hr(&e));
                }
            },
            Err(e) => {
                let _ = writeln!(r, "connect()                  : {}", hr(&e));
            }
        }
    }
    r
}

/// Experiment: find the exact `Put` combination accepted by `hpqBDataIn`.
pub fn put_lab() -> String {
    use std::fmt::Write as _;

    let mut r = String::new();
    let client = match BiosClient::connect() {
        Ok(c) => c,
        Err(e) => return format!("connect failed: {}\n", hr(&e)),
    };

    unsafe {
        // Qualifiers on hpqBDataIn.Command
        match client
            .data_class
            .GetPropertyQualifierSet(&BSTR::from("Command"))
        {
            Ok(qs) => {
                for q in ["Key", "Read", "Write", "ValueMap", "Values", "CIMTYPE"] {
                    let mut v = VARIANT::default();
                    let mut flavor = 0i32;
                    let present = qs.Get(&BSTR::from(q), 0, &mut v, &mut flavor).is_ok();
                    let _ = writeln!(r, "qualifier {q:<10}: {}", if present { "present" } else { "-" });
                }
            }
            Err(e) => {
                let _ = writeln!(r, "qualifiers           : {}", hr(&e));
            }
        }

        let data_inst = match client.data_class.SpawnInstance(0) {
            Ok(x) => x,
            Err(e) => return format!("{r}spawn failed: {}\n", hr(&e)),
        };

        let u = variant_from_u32(131080);
        let i = variant_from_i32(131080);
        let n = variant_null();
        let e0 = VARIANT::default();

        let tries: [(&str, &VARIANT, i32); 8] = [
            ("vt=UI4  type=19", &u, 19),
            ("vt=I4   type=19", &i, 19),
            ("vt=UI4  type=0", &u, 0),
            ("vt=I4   type=3", &i, 3),
            ("vt=I4   type=0", &i, 0),
            ("vt=UI4  type=0xFFF", &u, 0x0FFF),
            ("vt=NULL type=19", &n, 19),
            ("vt=EMPTY type=19", &e0, 19),
        ];
        for (label, v, t) in tries {
            let res = data_inst.Put(&BSTR::from("Command"), 0, v, t);
            let _ = writeln!(r, "Put(Command) {label:<20}: {}", status(&res));
        }
    }
    r
}
