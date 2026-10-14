//! OMEN Hub RS — a simple replacement for the OMEN Gaming Hub controls:
//! modes (with an automatic mode), fans, keyboard lighting, startup options.
//!
//! The hardware work runs in `omen_core::Controller` threads; this crate is
//! only the window, the notification-area icon and the settings.

mod instance;
mod tray;
mod ui;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use omen_core::{Config, Controller};

pub const APP_TITLE: &str = "OMEN Hub RS";

pub fn log_line(msg: &str) {
    omen_core::log_line(msg);
}

/// Red OMEN-style round icon.
pub(crate) fn icon_rgba(size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let c = size as f32 / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - c + 0.5;
            let dy = y as f32 - c + 0.5;
            let d = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            let ring = d <= c - 1.0 && d >= c * 0.55;
            let dot = d <= c * 0.3;
            if ring || dot {
                rgba[i] = 225;
                rgba[i + 1] = 30;
                rgba[i + 2] = 40;
                rgba[i + 3] = 255;
            }
        }
    }
    rgba
}

pub(crate) fn show_main_window(ctx: &egui::Context) {
    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
    ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
    instance::show_window(APP_TITLE);
    ctx.request_repaint();
}

/// Saves, returns fan control to the BIOS when needed, and exits.
pub(crate) fn quit(controller: &Controller, config: &Arc<Mutex<Config>>) -> ! {
    config.lock().unwrap_or_else(|e| e.into_inner()).save();
    controller.shutdown();
    log_line("omen-hub: quit");
    std::process::exit(0);
}

#[cfg(windows)]
pub(crate) fn relaunch_elevated() {
    use windows::core::HSTRING;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        if let Ok(exe) = std::env::current_exe() {
            ShellExecuteW(
                None,
                &HSTRING::from("runas"),
                &HSTRING::from(exe.to_string_lossy().to_string()),
                None,
                None,
                SW_SHOWNORMAL,
            );
        }
    }
}

pub fn run() -> Result<(), eframe::Error> {
    std::panic::set_hook(Box::new(|info| log_line(&format!("PANIC: {info}"))));

    let Some(_guard) = instance::acquire() else {
        log_line("omen-hub: already running, asked it to show");
        return Ok(());
    };
    log_line("omen-hub: starting");

    // Process COM security must be set before winit/OLE does it: the WMI
    // provider needs impersonation level (see docs/bios-wmi-commands.md).
    unsafe {
        use windows::Win32::System::Com::{
            CoInitializeEx, CoInitializeSecurity, COINIT_APARTMENTTHREADED, EOAC_NONE,
            RPC_C_AUTHN_LEVEL_DEFAULT, RPC_C_IMP_LEVEL_IMPERSONATE,
        };
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
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
    }

    let config = Config::load();
    let start_hidden = std::env::args().any(|a| a == "--tray") || config.start_minimized;

    let icon = egui::IconData {
        rgba: icon_rgba(64),
        width: 64,
        height: 64,
    };
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(APP_TITLE)
            .with_inner_size([1000.0, 720.0])
            .with_min_inner_size([820.0, 600.0])
            .with_visible(!start_hidden)
            .with_icon(icon),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(move |cc| {
            let controller = Arc::new(Controller::start(config.clone()));
            let config = Arc::new(Mutex::new(config));
            {
                let ctx = cc.egui_ctx.clone();
                instance::listen(move || show_main_window(&ctx));
            }
            // Keep the window state fresh even when nothing is clicked.
            {
                let ctx = cc.egui_ctx.clone();
                std::thread::spawn(move || loop {
                    std::thread::sleep(Duration::from_millis(1000));
                    ctx.request_repaint();
                });
            }
            Ok(Box::new(ui::App::new(cc, controller, config, start_hidden)))
        }),
    )
}
