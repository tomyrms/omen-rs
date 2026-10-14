//! Notification-area icon and menu (works while the window is hidden).

use std::sync::{Arc, Mutex};

use omen_core::{Command, Config, Controller, Mode};
use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::{icon_rgba, APP_TITLE};

pub struct Tray {
    _icon: TrayIcon,
    modes: Vec<(Mode, CheckMenuItem)>,
    shown_mode: Option<Mode>,
    tooltip: String,
}

impl Tray {
    pub fn build(
        controller: Arc<Controller>,
        config: Arc<Mutex<Config>>,
        egui_ctx: egui::Context,
    ) -> Option<Self> {
        let menu = Menu::new();
        let show = MenuItem::new("Ouvrir OMEN Hub RS", true, None);
        let quit = MenuItem::new("Quitter", true, None);
        let modes: Vec<(Mode, CheckMenuItem)> = Mode::MAIN
            .iter()
            .map(|m| (*m, CheckMenuItem::new(format!("{} {}", m.icon(), m.label()), true, false, None)))
            .collect();
        let _ = menu.append(&show);
        let _ = menu.append(&PredefinedMenuItem::separator());
        for (_, item) in &modes {
            let _ = menu.append(item);
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&quit);

        let icon = tray_icon::Icon::from_rgba(icon_rgba(32), 32, 32).ok()?;
        let tray = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip(APP_TITLE)
            .with_icon(icon)
            .build()
            .ok()?;

        let show_id = show.id().clone();
        let quit_id = quit.id().clone();
        let mode_ids: Vec<(Mode, tray_icon::menu::MenuId)> =
            modes.iter().map(|(m, i)| (*m, i.id().clone())).collect();
        {
            let controller = controller.clone();
            let ctx = egui_ctx.clone();
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                if event.id == show_id {
                    crate::show_main_window(&ctx);
                } else if event.id == quit_id {
                    crate::quit(&controller, &config);
                } else if let Some((mode, _)) = mode_ids.iter().find(|(_, id)| *id == event.id) {
                    controller.send(Command::SetMode(*mode));
                    let mut cfg = config.lock().unwrap_or_else(|e| e.into_inner());
                    cfg.mode = *mode;
                    cfg.save();
                    ctx.request_repaint();
                }
            }));
        }
        TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                crate::show_main_window(&egui_ctx);
            }
        }));

        Some(Self {
            _icon: tray,
            modes,
            shown_mode: None,
            tooltip: String::new(),
        })
    }

    /// Keeps the check marks and tooltip in sync with the current mode.
    pub fn sync(&mut self, mode: Mode, tooltip: &str) {
        if self.shown_mode != Some(mode) {
            for (m, item) in &self.modes {
                item.set_checked(*m == mode);
            }
            self.shown_mode = Some(mode);
        }
        if self.tooltip != tooltip {
            self.tooltip = tooltip.to_string();
            let _ = self._icon.set_tooltip(Some(tooltip));
        }
    }
}
