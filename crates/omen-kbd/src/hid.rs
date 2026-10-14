//! Minimal overlapped HID transport (enumerate, open, write, read with timeout).

use std::time::{Duration, Instant};

use windows::core::{Error, Result, PCWSTR};
use windows::Win32::Devices::DeviceAndDriverInstallation::{
    CM_Get_Device_Interface_ListW, CM_Get_Device_Interface_List_SizeW,
    CM_GET_DEVICE_INTERFACE_LIST_PRESENT, CR_SUCCESS,
};
use windows::Win32::Devices::HumanInterfaceDevice::{
    HidD_FreePreparsedData, HidD_GetAttributes, HidD_GetHidGuid, HidD_GetPreparsedData,
    HidP_GetCaps, HIDD_ATTRIBUTES, HIDP_CAPS, PHIDP_PREPARSED_DATA,
};
use windows::Win32::Foundation::{
    CloseHandle, ERROR_IO_PENDING, E_FAIL, GENERIC_READ, GENERIC_WRITE, HANDLE, WAIT_OBJECT_0,
};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, WriteFile, FILE_FLAG_OVERLAPPED, FILE_SHARE_READ, FILE_SHARE_WRITE,
    OPEN_EXISTING,
};
use windows::Win32::System::Threading::{CreateEventW, WaitForSingleObject};
use windows::Win32::System::IO::{CancelIoEx, GetOverlappedResult, OVERLAPPED};

/// A HID device interface found on the system.
#[derive(Debug, Clone)]
pub struct HidInfo {
    pub path: String,
    pub vendor_id: u16,
    pub product_id: u16,
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Lists present HID interfaces whose path contains `filter` (case-insensitive).
pub fn enumerate(filter: &str) -> Vec<HidInfo> {
    let filter = filter.to_ascii_lowercase();
    let mut out = Vec::new();
    unsafe {
        let guid = HidD_GetHidGuid();
        let mut len = 0u32;
        if CM_Get_Device_Interface_List_SizeW(
            &mut len,
            &guid,
            PCWSTR::null(),
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        ) != CR_SUCCESS
            || len == 0
        {
            return out;
        }
        let mut buf = vec![0u16; len as usize];
        if CM_Get_Device_Interface_ListW(
            &guid,
            PCWSTR::null(),
            &mut buf,
            CM_GET_DEVICE_INTERFACE_LIST_PRESENT,
        ) != CR_SUCCESS
        {
            return out;
        }
        for path in buf.split(|c| *c == 0).filter(|p| !p.is_empty()) {
            let path = String::from_utf16_lossy(path);
            if !path.to_ascii_lowercase().contains(&filter) {
                continue;
            }
            // Attribute query needs no access rights.
            let w = wide(&path);
            let Ok(h) = CreateFileW(
                PCWSTR(w.as_ptr()),
                0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                Default::default(),
                None,
            ) else {
                continue;
            };
            let mut attr = HIDD_ATTRIBUTES {
                Size: std::mem::size_of::<HIDD_ATTRIBUTES>() as u32,
                ..Default::default()
            };
            if HidD_GetAttributes(h, &mut attr) {
                out.push(HidInfo {
                    path,
                    vendor_id: attr.VendorID,
                    product_id: attr.ProductID,
                });
            }
            let _ = CloseHandle(h);
        }
    }
    out
}

/// An open HID interface with overlapped read/write.
pub struct HidDevice {
    handle: HANDLE,
    event: HANDLE,
    pub output_len: usize,
    pub input_len: usize,
    pub path: String,
}

// The handles are only used through &mut self or immutable reads of plain
// data; moving the device between threads is safe.
unsafe impl Send for HidDevice {}

impl HidDevice {
    pub fn open(path: &str) -> Result<Self> {
        unsafe {
            let w = wide(path);
            let handle = CreateFileW(
                PCWSTR(w.as_ptr()),
                (GENERIC_READ | GENERIC_WRITE).0,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                None,
                OPEN_EXISTING,
                FILE_FLAG_OVERLAPPED,
                None,
            )?;
            let event = match CreateEventW(None, true, false, PCWSTR::null()) {
                Ok(e) => e,
                Err(e) => {
                    let _ = CloseHandle(handle);
                    return Err(e);
                }
            };
            let (mut output_len, mut input_len) = (65usize, 65usize);
            let mut pp = PHIDP_PREPARSED_DATA::default();
            if HidD_GetPreparsedData(handle, &mut pp) {
                let mut caps = HIDP_CAPS::default();
                if HidP_GetCaps(pp, &mut caps).is_ok() {
                    if caps.OutputReportByteLength > 0 {
                        output_len = caps.OutputReportByteLength as usize;
                    }
                    if caps.InputReportByteLength > 0 {
                        input_len = caps.InputReportByteLength as usize;
                    }
                }
                let _ = HidD_FreePreparsedData(pp);
            }
            Ok(Self {
                handle,
                event,
                output_len,
                input_len,
                path: path.to_string(),
            })
        }
    }

    fn wait(&self, ov: &mut OVERLAPPED, timeout: Duration) -> Result<Option<usize>> {
        unsafe {
            let r = WaitForSingleObject(self.event, timeout.as_millis() as u32);
            if r != WAIT_OBJECT_0 {
                let _ = CancelIoEx(self.handle, Some(ov));
                // Let the cancellation complete before `ov` goes away.
                let mut n = 0u32;
                let _ = GetOverlappedResult(self.handle, ov, &mut n, true);
                return Ok(None);
            }
            let mut n = 0u32;
            GetOverlappedResult(self.handle, ov, &mut n, false)?;
            Ok(Some(n as usize))
        }
    }

    /// Writes one output report (padded/truncated to the device report size).
    pub fn write(&mut self, report: &[u8]) -> Result<()> {
        let mut buf = vec![0u8; self.output_len];
        let n = report.len().min(buf.len());
        buf[..n].copy_from_slice(&report[..n]);
        unsafe {
            let mut ov = OVERLAPPED {
                hEvent: self.event,
                ..Default::default()
            };
            windows::Win32::System::Threading::ResetEvent(self.event)?;
            if let Err(e) = WriteFile(self.handle, Some(&buf), None, Some(&mut ov)) {
                if e.code() != ERROR_IO_PENDING.to_hresult() {
                    return Err(e);
                }
            }
            match self.wait(&mut ov, Duration::from_millis(1000))? {
                Some(_) => Ok(()),
                None => Err(Error::new(E_FAIL, "HID write timeout")),
            }
        }
    }

    /// Reads one input report, or `None` on timeout.
    pub fn read(&mut self, timeout: Duration) -> Result<Option<Vec<u8>>> {
        let mut buf = vec![0u8; self.input_len];
        unsafe {
            let mut ov = OVERLAPPED {
                hEvent: self.event,
                ..Default::default()
            };
            windows::Win32::System::Threading::ResetEvent(self.event)?;
            if let Err(e) = ReadFile(self.handle, Some(&mut buf), None, Some(&mut ov)) {
                if e.code() != ERROR_IO_PENDING.to_hresult() {
                    return Err(e);
                }
            }
            Ok(self.wait(&mut ov, timeout)?.map(|n| {
                buf.truncate(n);
                buf
            }))
        }
    }

    /// Discards pending input reports (key auto-reports, stale answers).
    pub fn drain(&mut self) {
        let start = Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match self.read(Duration::from_millis(5)) {
                Ok(Some(_)) => continue,
                _ => break,
            }
        }
    }
}

impl Drop for HidDevice {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.event);
            let _ = CloseHandle(self.handle);
        }
    }
}
