//! BIOS hotkey events (`root\WMI` `hpqBEvnt`).
//!
//! The firmware raises `hpqBEvnt` for hotkeys; the OMEN key is
//! `EventID = 29`, `EventData = 8613` (0x21A5).

use windows::core::{Result as WinResult, BSTR};
use windows::Win32::System::Wmi::{
    IEnumWbemClassObject, IWbemClassObject, WBEM_FLAG_FORWARD_ONLY, WBEM_FLAG_RETURN_IMMEDIATELY,
    WBEM_GENERIC_FLAG_TYPE, WBEM_S_TIMEDOUT,
};

use crate::{connect_services, step, variant_as_i32, OwnedVariant};

/// One `hpqBEvnt` notification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BiosEvent {
    pub id: u32,
    pub data: u32,
}

impl BiosEvent {
    pub const HOTKEY: u32 = 29;
    pub const OMEN_KEY: u32 = 8613;

    pub fn is_omen_key(&self) -> bool {
        self.id == Self::HOTKEY && self.data == Self::OMEN_KEY
    }
}

/// Subscription to BIOS events. Owned by the thread that created it.
pub struct BiosEvents {
    events: IEnumWbemClassObject,
}

impl BiosEvents {
    pub fn subscribe() -> WinResult<Self> {
        unsafe {
            let services = connect_services()?;
            let flags = WBEM_GENERIC_FLAG_TYPE(WBEM_FLAG_RETURN_IMMEDIATELY.0 | WBEM_FLAG_FORWARD_ONLY.0);
            let events = services
                .ExecNotificationQuery(&BSTR::from("WQL"), &BSTR::from("SELECT * FROM hpqBEvnt"), flags, None)
                .map_err(|e| step("ExecNotificationQuery(hpqBEvnt)", e))?;
            Ok(Self { events })
        }
    }

    /// Waits up to `timeout_ms` for the next event.
    pub fn next(&self, timeout_ms: i32) -> WinResult<Option<BiosEvent>> {
        unsafe {
            let mut objs: [Option<IWbemClassObject>; 1] = [None];
            let mut returned = 0u32;
            let hr = self.events.Next(timeout_ms, &mut objs, &mut returned);
            if hr.0 == WBEM_S_TIMEDOUT.0 || returned == 0 {
                hr.ok()?;
                return Ok(None);
            }
            hr.ok()?;
            let Some(obj) = objs[0].take() else { return Ok(None) };
            let read = |name: &str| {
                let mut v = OwnedVariant::empty();
                obj.Get(&BSTR::from(name), 0, &mut v.0, None, None)
                    .ok()
                    .and_then(|_| variant_as_i32(&v.0))
                    .unwrap_or(-1) as u32
            };
            Ok(Some(BiosEvent { id: read("EventID"), data: read("EventData") }))
        }
    }
}
