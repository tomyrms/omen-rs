//! Single instance: a second launch (double-click while the app sits in the
//! notification area) just brings the running window back.

use windows::core::PCWSTR;
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE, HWND};
use windows::Win32::System::Threading::{
    CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE,
    INFINITE,
};
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};

const MUTEX_NAME: &str = "Local\\OmenHubRS-SingleInstance";
const EVENT_NAME: &str = "Local\\OmenHubRS-Show";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Returns `false` when another instance already runs (it has been asked to
/// show its window). Keep the returned guard alive for the process lifetime.
pub fn acquire() -> Option<Guard> {
    unsafe {
        let name = wide(MUTEX_NAME);
        let mutex = CreateMutexW(None, false, PCWSTR(name.as_ptr())).ok()?;
        if GetLastError() == ERROR_ALREADY_EXISTS {
            let ev = wide(EVENT_NAME);
            if let Ok(h) = OpenEventW(EVENT_MODIFY_STATE, false, PCWSTR(ev.as_ptr())) {
                let _ = SetEvent(h);
            }
            return None;
        }
        Some(Guard { _mutex: mutex })
    }
}

pub struct Guard {
    _mutex: HANDLE,
}

/// Calls `on_show` every time another instance asks for the window.
pub fn listen(on_show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || unsafe {
        let ev = wide(EVENT_NAME);
        let Ok(h) = CreateEventW(None, false, false, PCWSTR(ev.as_ptr())) else {
            return;
        };
        loop {
            WaitForSingleObject(h, INFINITE);
            on_show();
        }
    });
}

/// Shows and focuses the main window, even when it was hidden to the tray.
pub fn show_window(title: &str) {
    unsafe {
        let t = wide(title);
        if let Ok(hwnd) = FindWindowW(PCWSTR::null(), PCWSTR(t.as_ptr())) {
            if hwnd != HWND::default() {
                let _ = ShowWindow(hwnd, SW_SHOW);
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}
