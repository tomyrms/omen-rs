//! The window: Home (modes + live status), Keyboard, Settings, Advanced.

use std::sync::{Arc, Mutex, MutexGuard};

use egui::{Align, Color32, CornerRadius, Layout, RichText, Sense, Stroke, Vec2};
use egui_plot::{Legend, Line, Plot, PlotPoints};
use omen_core::config::KeyboardLook;
use omen_core::vendor::{self, HubStatus};
use omen_core::{
    autostart, Command, Config, Controller, KeyboardCommand, Level, Mode, OmenKeyAction, Sensitivity,
    Snapshot,
};
use omen_kbd::{Direction, Effect, EffectSettings, Rgb, Theme};
use omen_wmi::{FanCurve, FanCurvePoint, GpuMode};

use crate::tray::Tray;
use crate::APP_TITLE;

const ACCENT: Color32 = Color32::from_rgb(225, 35, 45);
const ACCENT_DIM: Color32 = Color32::from_rgb(120, 25, 32);
const CARD: Color32 = Color32::from_rgb(30, 32, 38);
const MUTED: Color32 = Color32::from_rgb(150, 155, 165);
const OK: Color32 = Color32::from_rgb(70, 190, 110);
const WARN: Color32 = Color32::from_rgb(235, 160, 50);
const ERR: Color32 = Color32::from_rgb(235, 80, 80);

const SWATCHES: &[(&str, Rgb)] = &[
    ("Rouge", Rgb::new(255, 0, 0)),
    ("Orange", Rgb::new(255, 90, 0)),
    ("Jaune", Rgb::new(255, 200, 0)),
    ("Vert", Rgb::new(0, 255, 0)),
    ("Cyan", Rgb::new(0, 220, 255)),
    ("Bleu", Rgb::new(0, 0, 255)),
    ("Violet", Rgb::new(150, 0, 255)),
    ("Rose", Rgb::new(255, 40, 140)),
    ("Blanc", Rgb::new(255, 255, 255)),
];

/// OMEN Gaming Hub panel: state and the running uninstall/reinstall job.
#[derive(Default)]
struct HubPanel {
    status: Option<HubStatus>,
    busy: bool,
    progress: String,
    result: Option<(String, bool)>,
}

#[derive(PartialEq, Clone, Copy)]
enum HubConfirm {
    Uninstall,
    Reinstall,
}

#[derive(PartialEq, Clone, Copy)]
enum Tab {
    Home,
    Keyboard,
    Settings,
    Advanced,
}

pub struct App {
    controller: Arc<Controller>,
    config: Arc<Mutex<Config>>,
    tray: Option<Tray>,
    tab: Tab,
    // Keyboard editor state
    color: Rgb,
    brightness: u8,
    effect: EffectSettings,
    effect_custom_colors: bool,
    // Settings editor state
    autostart_on: bool,
    autostart_msg: Option<(String, bool)>,
    new_game: String,
    // Advanced editor state
    curve_edit: FanCurve,
    manual_rpm: u32,
    confirm_mux: Option<GpuMode>,
    quitting_hint_shown: bool,
    hub: Arc<Mutex<HubPanel>>,
    logo: egui::TextureHandle,
    hub_disable_services: bool,
    hub_confirm: Option<HubConfirm>,
    /// Hide the window after the first frame (start in the notification area).
    hide_on_start: bool,
}

fn rgb32(c: Rgb) -> Color32 {
    Color32::from_rgb(c.r, c.g, c.b)
}

fn card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .fill(CARD)
        .corner_radius(CornerRadius::same(12))
        .inner_margin(14)
        .show(ui, add)
        .inner
}

fn section(ui: &mut egui::Ui, title: &str) {
    ui.add_space(10.0);
    ui.label(RichText::new(title).size(17.0).strong());
    ui.add_space(4.0);
}

fn fmt_opt(v: Option<f32>, unit: &str) -> String {
    v.map(|x| format!("{x:.0}{unit}")).unwrap_or_else(|| "—".into())
}

impl App {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        controller: Arc<Controller>,
        config: Arc<Mutex<Config>>,
        start_hidden: bool,
    ) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.selection.bg_fill = ACCENT;
        visuals.hyperlink_color = ACCENT;
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT_DIM);
        visuals.panel_fill = Color32::from_rgb(20, 21, 25);
        cc.egui_ctx.set_visuals(visuals);
        cc.egui_ctx.all_styles_mut(|s| {
            s.spacing.button_padding = Vec2::new(10.0, 6.0);
            s.spacing.item_spacing = Vec2::new(8.0, 8.0);
        });

        let cfg = config.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let (color, brightness) = match &cfg.keyboard {
            KeyboardLook::Color { color, brightness } => (*color, *brightness),
            _ => (Rgb::new(255, 40, 40), 100),
        };
        let effect = match &cfg.keyboard {
            KeyboardLook::Effect(e) => e.clone(),
            _ => EffectSettings::default(),
        };
        let tray = Tray::build(controller.clone(), config.clone(), cc.egui_ctx.clone());
        if tray.is_none() {
            crate::log_line("tray: could not create the notification icon");
        }
        Self {
            effect_custom_colors: !effect.colors.is_empty(),
            controller,
            tray,
            // OMENRS_TAB=clavier|reglages|avance opens another tab (handy for docs/screenshots).
            tab: match std::env::var("OMENRS_TAB").unwrap_or_default().as_str() {
                "clavier" => Tab::Keyboard,
                "reglages" => Tab::Settings,
                "avance" => Tab::Advanced,
                _ => Tab::Home,
            },
            color,
            brightness,
            effect,
            autostart_on: autostart::is_enabled(),
            autostart_msg: None,
            new_game: String::new(),
            curve_edit: cfg.custom_curve.clone(),
            manual_rpm: 3500,
            confirm_mux: None,
            quitting_hint_shown: false,
            hub: Arc::new(Mutex::new(HubPanel::default())),
            logo: cc.egui_ctx.load_texture(
                "logo",
                egui::ColorImage::from_rgba_unmultiplied([256, 256], crate::LOGO_256),
                egui::TextureOptions::LINEAR,
            ),
            hub_disable_services: true,
            hub_confirm: None,
            hide_on_start: start_hidden,
            config,
        }
    }

    fn hub(&self) -> MutexGuard<'_, HubPanel> {
        self.hub.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Runs a Hub job (or only a status refresh) off the UI thread.
    fn hub_job(&self, ctx: &egui::Context, job: Option<HubConfirm>, disable_services: bool) {
        {
            let mut h = self.hub();
            if h.busy {
                return;
            }
            h.busy = true;
            h.progress = if job.is_some() { "Préparation…".into() } else { "Lecture de l'état…".into() };
            if job.is_some() {
                h.result = None;
            }
        }
        let hub = self.hub.clone();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let set = |f: &dyn Fn(&mut HubPanel)| {
                f(&mut hub.lock().unwrap_or_else(|e| e.into_inner()));
                ctx.request_repaint();
            };
            let mut progress = |m: &str| {
                let m = m.to_string();
                set(&|h| h.progress = m.clone());
            };
            let result = match job {
                Some(HubConfirm::Uninstall) => Some(vendor::uninstall(disable_services, &mut progress).map(|()| {
                    "OMEN Gaming Hub est désinstallé. L'état d'avant est sauvegardé : « Réinstaller comme avant » le remettra tel quel.".to_string()
                })),
                Some(HubConfirm::Reinstall) => Some(vendor::reinstall(&mut progress)),
                None => None,
            };
            let status = vendor::status();
            set(&|h| {
                h.status = Some(status.clone());
                h.busy = false;
                h.progress.clear();
                if let Some(r) = &result {
                    h.result = Some(match r {
                        Ok(m) => (m.clone(), false),
                        Err(e) => (format!("Échec : {e}"), true),
                    });
                }
            });
        });
    }

    fn cfg(&self) -> MutexGuard<'_, Config> {
        self.config.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn set_mode(&self, mode: Mode) {
        self.controller.send(Command::SetMode(mode));
        let mut cfg = self.cfg();
        cfg.mode = mode;
        cfg.save();
    }

    fn apply_keyboard(&self, look: KeyboardLook) {
        self.controller.send(Command::Keyboard(KeyboardCommand::Apply(look.clone())));
        let mut cfg = self.cfg();
        cfg.keyboard = look;
        cfg.save();
    }

    // -- header / banners ---------------------------------------------------

    fn header(&mut self, ui: &mut egui::Ui, s: &Snapshot) {
        ui.horizontal(|ui| {
            ui.add(egui::Image::new(&self.logo).fit_to_exact_size(Vec2::splat(34.0)));
            ui.label(RichText::new("OMEN").size(26.0).strong().color(ACCENT));
            ui.label(RichText::new("Hub RS").size(26.0).strong());
            ui.add_space(12.0);
            let chip = |ui: &mut egui::Ui, ok: bool, text: &str| {
                let color = if ok { OK } else { WARN };
                ui.label(RichText::new(format!("• {text}")).color(color).size(13.0));
            };
            chip(ui, s.elevated, if s.elevated { "Administrateur" } else { "Pas administrateur" });
            chip(ui, s.connected, if s.connected { "BIOS connecté" } else { "BIOS non connecté" });
            match s.keyboard.available {
                Some(true) => chip(ui, true, "Clavier RGB"),
                Some(false) => chip(ui, false, "Clavier non détecté"),
                None => {}
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if let Some(level) = s.applied {
                    let txt = if s.mode == Mode::Auto {
                        format!("Auto ({})", level.label())
                    } else {
                        s.mode.label().to_string()
                    };
                    ui.label(RichText::new(txt).size(15.0).strong());
                    ui.label(RichText::new("Mode actif :").color(MUTED));
                }
            });
        });

        if !s.elevated {
            egui::Frame::new().fill(Color32::from_rgb(70, 45, 15)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("⚠ Les modes et les ventilateurs nécessitent les droits administrateur.").color(WARN));
                    if ui.button("Relancer en administrateur").clicked() {
                        crate::relaunch_elevated();
                        crate::quit(&self.controller, &self.config);
                    }
                });
            });
        } else if let Some(err) = &s.connect_error {
            ui.colored_label(ERR, format!("⚠ {err}"));
        }
        if s.vendor_app_running {
            egui::Frame::new().fill(Color32::from_rgb(70, 45, 15)).corner_radius(8).inner_margin(10).show(ui, |ui| {
                ui.label(RichText::new(
                    "⚠ OMEN Gaming Hub est ouvert : il peut modifier les mêmes réglages et annuler les vôtres. Fermez-le (et son icône) pour que OMEN Hub RS garde la main.",
                ).color(WARN));
            });
        }
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            for (tab, label) in [
                (Tab::Home, "🏠  Accueil"),
                (Tab::Keyboard, "⌨  Clavier"),
                (Tab::Settings, "⚙  Réglages"),
                (Tab::Advanced, "🔧  Avancé"),
            ] {
                let selected = self.tab == tab;
                let text = RichText::new(label).size(15.0);
                let text = if selected { text.strong().color(Color32::WHITE) } else { text.color(MUTED) };
                let btn = egui::Button::new(text)
                    .fill(if selected { ACCENT_DIM } else { Color32::TRANSPARENT })
                    .corner_radius(8)
                    .min_size(Vec2::new(120.0, 34.0));
                if ui.add(btn).clicked() {
                    self.tab = tab;
                }
            }
        });
        ui.separator();
    }

    // -- Home ---------------------------------------------------------------

    fn home(&mut self, ui: &mut egui::Ui, s: &Snapshot) {
        // Live status cards
        ui.columns(4, |cols| {
            card(&mut cols[0], |ui| {
                ui.label(RichText::new("Processeur").color(MUTED));
                ui.label(RichText::new(fmt_opt(s.cpu_load, " %")).size(28.0).strong());
                ui.label(format!("Température : {}", fmt_opt(s.cpu_temp(), " °C")));
            });
            card(&mut cols[1], |ui| {
                ui.label(RichText::new("Carte graphique").color(MUTED));
                if s.gpu_sensors_paused {
                    ui.label(RichText::new("veille").size(28.0).strong());
                    ui.label("Lecture coupée sur batterie");
                } else {
                    ui.label(RichText::new(fmt_opt(s.gpu.temp, " °C")).size(28.0).strong());
                    ui.label(format!(
                        "Charge {} · {}",
                        fmt_opt(s.gpu_load, " %"),
                        fmt_opt(s.gpu.power_w, " W")
                    ));
                }
            });
            card(&mut cols[2], |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Ventilateurs").color(MUTED));
                    if s.max_fan == Some(true) {
                        ui.label(RichText::new("MAX").color(ACCENT).strong());
                    }
                });
                let rpm = |v: Option<u8>| {
                    Snapshot::fan_rpm(v).map(|r| format!("{r} tr/min")).unwrap_or_else(|| "—".into())
                };
                ui.label(RichText::new(rpm(s.fan_cpu)).size(22.0).strong());
                ui.label(format!("GPU : {}", rpm(s.fan_gpu)));
            });
            card(&mut cols[3], |ui| {
                ui.label(RichText::new("Alimentation").color(MUTED));
                let big = if s.power.on_ac { "Secteur" } else { "Batterie" };
                ui.label(RichText::new(big).size(24.0).strong());
                let mut line = s.power.battery_percent.map(|p| format!("{p} %")).unwrap_or_default();
                if let Some(a) = s.adapter.filter(|a| a.watts > 0 && s.power.on_ac) {
                    line = format!("{line} · chargeur {} W", a.watts);
                }
                ui.label(line);
            });
        });

        section(ui, "Mode");
        let width = (ui.available_width() - 4.0 * 8.0) / 5.0;
        ui.horizontal(|ui| {
            for mode in Mode::MAIN {
                let selected = s.mode == mode;
                let fill = if selected { ACCENT } else { CARD };
                let resp = egui::Frame::new()
                    .fill(fill)
                    .corner_radius(CornerRadius::same(14))
                    .inner_margin(12)
                    .show(ui, |ui| {
                        ui.set_width(width - 24.0);
                        ui.set_height(92.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new(mode.icon()).size(26.0));
                            ui.label(RichText::new(mode.label()).size(17.0).strong().color(Color32::WHITE));
                            if mode == Mode::Auto {
                                ui.label(RichText::new("recommandé").size(12.0).color(if selected { Color32::WHITE } else { MUTED }));
                            }
                        });
                    })
                    .response
                    .interact(Sense::click())
                    .on_hover_text(mode.description());
                if resp.hovered() && !selected {
                    ui.painter().rect_stroke(resp.rect, 14, Stroke::new(1.5, ACCENT_DIM), egui::StrokeKind::Inside);
                }
                if resp.clicked() {
                    self.set_mode(mode);
                }
            }
        });

        ui.add_space(6.0);
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            match s.mode {
                _ if !s.connected => {
                    ui.label(RichText::new(format!("Mode {} : en attente", s.mode.label())).size(16.0).strong());
                    ui.label("Les modes s'appliqueront dès que l'application aura les droits administrateur.");
                }
                Mode::Auto => {
                    let level = s.applied.map(Level::label).unwrap_or("…");
                    ui.label(RichText::new(format!("Mode Auto : {level}")).size(16.0).strong());
                    ui.label(format!("Pourquoi : {}", if s.auto_reason.is_empty() { "analyse en cours" } else { &s.auto_reason }));
                    ui.label(RichText::new(match &s.game {
                        Some(g) => format!("🎮 Jeu détecté : {g}"),
                        None => "🎮 Aucun jeu détecté".into(),
                    }).color(MUTED));
                }
                Mode::Custom => {
                    ui.label(RichText::new("Courbe perso active").size(16.0).strong());
                    if let Some((c, g)) = s.custom_levels {
                        ui.label(format!(
                            "Ventilateurs demandés : CPU {} tr/min, GPU {} tr/min",
                            omen_wmi::level_to_rpm(c),
                            omen_wmi::level_to_rpm(g)
                        ));
                    }
                }
                m => {
                    ui.label(RichText::new(m.label()).size(16.0).strong());
                    ui.label(m.description());
                }
            }
            if let Some(m) = s.manual_fan {
                ui.colored_label(WARN, format!("Ventilateurs fixés manuellement à ~{} tr/min (onglet Avancé)", omen_wmi::level_to_rpm(m)));
            }
        });

        if !s.message.is_empty() {
            ui.colored_label(if s.message_is_error { ERR } else { OK }, &s.message);
        }

        ui.add_space(4.0);
        egui::CollapsingHeader::new(RichText::new("Historique (10 dernières minutes)").size(15.0))
            .default_open(false)
            .show(ui, |ui| self.history(ui, s));
    }

    fn history(&self, ui: &mut egui::Ui, s: &Snapshot) {
        let pts = |f: &dyn Fn(&omen_core::HistoryPoint) -> Option<f64>| -> PlotPoints {
            s.history.iter().filter_map(|p| f(p).map(|v| [p.t, v])).collect()
        };
        Plot::new("history_temp")
            .legend(Legend::default())
            .height(170.0)
            .allow_scroll(false)
            .include_y(0.0)
            .include_y(100.0)
            .show(ui, |p| {
                p.line(Line::new("Charge CPU %", pts(&|h| Some(h.cpu_load as f64))));
                p.line(Line::new("Temp. GPU °C", pts(&|h| h.gpu_temp.map(|v| v as f64))));
            });
        Plot::new("history_fans")
            .legend(Legend::default())
            .height(150.0)
            .allow_scroll(false)
            .include_y(0.0)
            .show(ui, |p| {
                p.line(Line::new("Ventilateur CPU tr/min", pts(&|h| h.fan_cpu_rpm.map(|v| v as f64))));
                p.line(Line::new("Ventilateur GPU tr/min", pts(&|h| h.fan_gpu_rpm.map(|v| v as f64))));
            });
    }

    // -- Keyboard -----------------------------------------------------------

    fn keyboard(&mut self, ui: &mut egui::Ui, s: &Snapshot) {
        let kb = &s.keyboard;
        if kb.available == Some(false) && !kb.busy {
            card(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new("Clavier RGB non détecté").size(16.0).strong());
                ui.label("Ce PC n'expose pas le contrôleur d'éclairage OMEN (USB 0461, interface MI_02).");
                if ui.button("Réessayer").clicked() {
                    self.controller.send(Command::Keyboard(KeyboardCommand::Detect));
                }
            });
            return;
        }

        section(ui, "Couleur unie");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for (name, c) in SWATCHES {
                    let selected = self.color == *c;
                    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::click());
                    ui.painter().rect_filled(rect, 8, rgb32(*c));
                    if selected {
                        ui.painter().rect_stroke(rect, 8, Stroke::new(3.0, Color32::WHITE), egui::StrokeKind::Outside);
                    }
                    let resp = resp.on_hover_text(*name);
                    if resp.clicked() {
                        self.color = *c;
                        self.apply_keyboard(KeyboardLook::Color { color: *c, brightness: self.brightness });
                    }
                }
                ui.add_space(10.0);
                let mut arr = [self.color.r, self.color.g, self.color.b];
                ui.label("Autre :");
                if ui.color_edit_button_srgb(&mut arr).changed() {
                    self.color = Rgb::new(arr[0], arr[1], arr[2]);
                }
            });
            ui.horizontal(|ui| {
                ui.label("Intensité");
                ui.add(egui::Slider::new(&mut self.brightness, 5..=100).suffix(" %"));
                if ui
                    .add(egui::Button::new(RichText::new("Appliquer la couleur").strong()).fill(ACCENT_DIM))
                    .clicked()
                {
                    self.apply_keyboard(KeyboardLook::Color { color: self.color, brightness: self.brightness });
                }
            });
            ui.label(RichText::new(
                "La luminosité globale se règle avec la touche de rétroéclairage du clavier (Fn) ; le curseur atténue la couleur envoyée.",
            ).size(12.0).color(MUTED));
        });

        section(ui, "Effets animés");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                for e in Effect::ALL {
                    if ui.selectable_label(self.effect.effect == e, e.label()).clicked() {
                        self.effect.effect = e;
                    }
                }
            });
            ui.horizontal(|ui| {
                ui.label("Vitesse");
                for (v, l) in [(0u8, "Lente"), (1, "Moyenne"), (2, "Rapide")] {
                    if ui.selectable_label(self.effect.speed == v, l).clicked() {
                        self.effect.speed = v;
                    }
                }
                if self.effect.effect.has_direction() {
                    ui.separator();
                    ui.label("Direction");
                    egui::ComboBox::from_id_salt("dir")
                        .selected_text(self.effect.direction.label())
                        .show_ui(ui, |ui| {
                            for d in Direction::ALL {
                                ui.selectable_value(&mut self.effect.direction, d, d.label());
                            }
                        });
                }
            });
            ui.horizontal(|ui| {
                ui.label("Couleurs");
                ui.radio_value(&mut self.effect_custom_colors, false, "Thème");
                ui.radio_value(&mut self.effect_custom_colors, true, "Mes couleurs");
                if !self.effect_custom_colors {
                    for t in Theme::ALL {
                        if ui.selectable_label(self.effect.theme == t, t.label()).clicked() {
                            self.effect.theme = t;
                        }
                    }
                } else {
                    if self.effect.colors.is_empty() {
                        self.effect.colors = vec![Rgb::new(255, 0, 0), Rgb::new(0, 0, 255)];
                    }
                    let mut remove = None;
                    for (i, c) in self.effect.colors.iter_mut().enumerate() {
                        let mut arr = [c.r, c.g, c.b];
                        if ui.color_edit_button_srgb(&mut arr).changed() {
                            *c = Rgb::new(arr[0], arr[1], arr[2]);
                        }
                        if ui.small_button("✖").on_hover_text("Retirer").clicked() {
                            remove = Some(i);
                        }
                    }
                    if let Some(i) = remove {
                        if self.effect.colors.len() > 1 {
                            self.effect.colors.remove(i);
                        }
                    }
                    if self.effect.colors.len() < 4 && ui.button("+").on_hover_text("Ajouter une couleur").clicked() {
                        self.effect.colors.push(Rgb::new(0, 255, 0));
                    }
                }
            });
            if ui
                .add(egui::Button::new(RichText::new("Lancer l'effet").strong()).fill(ACCENT_DIM))
                .clicked()
            {
                let mut e = self.effect.clone();
                if !self.effect_custom_colors {
                    e.colors.clear();
                }
                self.apply_keyboard(KeyboardLook::Effect(e));
            }
        });

        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("💡 Éteindre le rétroéclairage").clicked() {
                self.apply_keyboard(KeyboardLook::Off);
            }
            if ui.button("Éclairage d'usine").clicked() {
                self.controller.send(Command::Keyboard(KeyboardCommand::RestoreDefault));
            }
            let mut restore = self.cfg().restore_keyboard_on_start;
            if ui.checkbox(&mut restore, "Réappliquer mon éclairage au lancement").changed() {
                let mut cfg = self.cfg();
                cfg.restore_keyboard_on_start = restore;
                cfg.save();
            }
        });
        if kb.busy {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Envoi au clavier…");
            });
        } else if !kb.message.is_empty() {
            ui.colored_label(if kb.error { ERR } else { OK }, &kb.message);
        }
        if !kb.model.is_empty() {
            ui.label(RichText::new(format!("{} · firmware {}", kb.model, kb.firmware)).size(12.0).color(MUTED));
        }
    }

    // -- Settings -----------------------------------------------------------

    fn settings(&mut self, ui: &mut egui::Ui, s: &Snapshot) {
        section(ui, "Démarrage");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut on = self.autostart_on;
            if ui
                .checkbox(&mut on, RichText::new("Lancer OMEN Hub RS au démarrage de Windows").size(15.0))
                .changed()
            {
                let r = if on { autostart::enable() } else { autostart::disable() };
                self.autostart_on = autostart::is_enabled();
                self.autostart_msg = Some(match r {
                    Ok(()) if on => ("Activé : l'application démarrera, réduite, à l'ouverture de session (sans demande de confirmation).".into(), false),
                    Ok(()) => ("Désactivé.".into(), false),
                    Err(e) => (format!("Impossible : {e}"), true),
                });
            }
            if let Some((m, err)) = &self.autostart_msg {
                ui.colored_label(if *err { ERR } else { OK }, m);
            }
            let mut cfg = self.cfg().clone();
            let before = cfg.clone();
            ui.checkbox(&mut cfg.start_minimized, "Démarrer réduit dans la zone de notification (près de l'horloge)");
            ui.checkbox(&mut cfg.close_to_tray, "Fermer la fenêtre garde l'application active en arrière-plan");
            if cfg != before {
                cfg.save();
                *self.cfg() = cfg;
            }
        });

        section(ui, "Touche OMEN");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label("Action de la touche OMEN (au-dessus du clavier) tant que OMEN Hub RS tourne :");
            let mut key = self.cfg().omen_key;
            ui.horizontal_wrapped(|ui| {
                for a in OmenKeyAction::ALL {
                    ui.radio_value(&mut key, a, a.label());
                }
            });
            if key != self.cfg().omen_key {
                let mut cfg = self.cfg();
                cfg.omen_key = key;
                cfg.save();
            }
            let hub_installed = self.hub().status.as_ref().is_some_and(|h| h.version.is_some());
            if hub_installed {
                ui.label(RichText::new("OMEN Gaming Hub est encore installé : il ouvrira aussi sa propre fenêtre à chaque appui.").size(12.0).color(WARN));
            }
        });

        self.hub_section(ui);

        section(ui, "Mode Auto");
        let mut auto = self.cfg().auto.clone();
        let before = auto.clone();
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label("Réactivité :");
                for sens in [Sensitivity::Calm, Sensitivity::Normal, Sensitivity::Reactive] {
                    ui.radio_value(&mut auto.sensitivity, sens, sens.label());
                }
            });
            ui.label(RichText::new("Discret = privilégie le silence · Réactif = refroidit plus tôt et plus longtemps").size(12.0).color(MUTED));
            ui.checkbox(&mut auto.performance_for_games, "Passer en Performance quand un jeu est lancé");
            ui.add_enabled(
                auto.performance_for_games,
                egui::Checkbox::new(&mut auto.games_only_on_ac, "… seulement quand le PC est branché"),
            );
            ui.checkbox(&mut auto.max_fans_when_hot, "Ventilateurs à fond si le PC devient trop chaud");
            ui.checkbox(&mut auto.silent_when_idle, "Mode Silencieux quand le PC ne fait rien");
        });

        section(ui, "Jeux");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label("Les jeux Steam, Epic, GOG, EA, Ubisoft, Riot, Xbox et Battle.net sont reconnus automatiquement, ainsi que tout jeu en plein écran.");
            match &s.game {
                Some(g) => {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(format!("🎮 En ce moment : {g}")).strong());
                        if ui.small_button("Ce n'est pas un jeu").clicked() && !auto.ignored_games.contains(g) {
                            auto.ignored_games.push(g.clone());
                        }
                    });
                }
                None => {
                    ui.label(RichText::new("🎮 Aucun jeu détecté en ce moment").color(MUTED));
                }
            }
            ui.horizontal(|ui| {
                ui.label("Ajouter un jeu (nom du .exe) :");
                ui.add(egui::TextEdit::singleline(&mut self.new_game).hint_text("monjeu.exe").desired_width(180.0));
                if ui.button("Ajouter").clicked() {
                    let mut name = self.new_game.trim().to_ascii_lowercase();
                    if !name.is_empty() {
                        if !name.ends_with(".exe") {
                            name.push_str(".exe");
                        }
                        if !auto.extra_games.contains(&name) {
                            auto.extra_games.push(name);
                        }
                        self.new_game.clear();
                    }
                }
            });
            let mut remove_extra = None;
            for (i, g) in auto.extra_games.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("✔ {g}"));
                    if ui.small_button("✖").clicked() {
                        remove_extra = Some(i);
                    }
                });
            }
            if let Some(i) = remove_extra {
                auto.extra_games.remove(i);
            }
            let mut remove_ignored = None;
            for (i, g) in auto.ignored_games.iter().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(RichText::new(format!("✖ ignoré : {g}")).color(MUTED));
                    if ui.small_button("rétablir").clicked() {
                        remove_ignored = Some(i);
                    }
                });
            }
            if let Some(i) = remove_ignored {
                auto.ignored_games.remove(i);
            }
        });
        if auto != before {
            self.controller.send(Command::SetAutoSettings(auto.clone()));
            let mut cfg = self.cfg();
            cfg.auto = auto;
            cfg.save();
        }

        section(ui, "Capteurs");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            let mut v = self.cfg().gpu_sensors_on_battery;
            if ui
                .checkbox(&mut v, "Lire la carte graphique NVIDIA aussi sur batterie")
                .on_hover_text("Interroger le GPU l'empêche de se mettre en veille et réduit l'autonomie.")
                .changed()
            {
                self.controller.send(Command::SetGpuSensorsOnBattery(v));
                let mut cfg = self.cfg();
                cfg.gpu_sensors_on_battery = v;
                cfg.save();
            }
        });
    }

    fn hub_section(&mut self, ui: &mut egui::Ui) {
        section(ui, "OMEN Gaming Hub (application HP)");
        let ctx = ui.ctx().clone();
        if self.hub().status.is_none() && !self.hub().busy {
            self.hub_job(&ctx, None, false);
        }
        let (status, busy, progress, result) = {
            let h = self.hub();
            (h.status.clone(), h.busy, h.progress.clone(), h.result.clone())
        };
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label("OMEN Hub RS remplace ses réglages : vous pouvez désinstaller l'application HP et ses services, puis tout remettre comme avant d'un clic. Les pilotes HP ne sont jamais touchés (l'interface BIOS en a besoin).");
            match &status {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("Lecture de l'état…");
                    });
                }
                Some(st) => {
                    let (txt, color) = match &st.version {
                        Some(v) => (format!("installé (version {v})"), WARN),
                        None => ("non installé".to_string(), OK),
                    };
                    ui.label(RichText::new(format!("Application : {txt}")).strong().color(color));
                    for sv in &st.services {
                        ui.label(format!(
                            "• {} : {}{}",
                            sv.display,
                            sv.start_label(),
                            if sv.running { ", en cours" } else { ", arrêté" }
                        ));
                    }
                    for t in &st.tasks {
                        ui.label(format!(
                            "• Tâche HP « {} » : {}{}",
                            t.label(),
                            if t.enabled { "activée" } else { "désactivée" },
                            if t.running { ", en cours" } else { "" }
                        ));
                    }
                    if st.has_backup {
                        ui.label(RichText::new("État d'origine sauvegardé : « Réinstaller comme avant » le restaurera.").size(12.0).color(MUTED));
                    }
                }
            }
            if !omen_core::controller::is_elevated() {
                ui.colored_label(WARN, "Les droits administrateur sont nécessaires pour ces actions.");
            }
            ui.add_space(4.0);
            ui.add_enabled_ui(!busy && status.is_some(), |ui| {
                let st = status.clone().unwrap_or_default();
                ui.horizontal(|ui| {
                    if st.anything_left()
                        && ui
                            .add(egui::Button::new(RichText::new("Désinstaller OMEN Gaming Hub").strong()).fill(ACCENT_DIM))
                            .clicked()
                    {
                        self.hub_confirm = Some(HubConfirm::Uninstall);
                    }
                    if (st.version.is_none() || st.has_backup) && ui.button("Réinstaller comme avant").clicked() {
                        self.hub_confirm = Some(HubConfirm::Reinstall);
                    }
                    if ui.button("Actualiser").clicked() {
                        self.hub_job(&ctx, None, false);
                    }
                });
                ui.checkbox(
                    &mut self.hub_disable_services,
                    "Désactiver aussi ses services et tâches en arrière-plan (HP Omen, System Info, Network, App Helper, overlay, Optimizer, surveillance)",
                );
            });
            if let Some(c) = self.hub_confirm {
                let text = match c {
                    HubConfirm::Uninstall => "Désinstaller OMEN Gaming Hub ? L'état actuel est sauvegardé avant.",
                    HubConfirm::Reinstall => "Rétablir les services et réinstaller OMEN Gaming Hub depuis le Microsoft Store ?",
                };
                ui.horizontal(|ui| {
                    ui.colored_label(WARN, text);
                    if ui.button("Confirmer").clicked() {
                        self.hub_confirm = None;
                        self.hub_job(&ctx, Some(c), self.hub_disable_services);
                    }
                    if ui.button("Annuler").clicked() {
                        self.hub_confirm = None;
                    }
                });
            }
            if busy {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(&progress);
                });
            } else if let Some((m, err)) = &result {
                ui.colored_label(if *err { ERR } else { OK }, m);
            }
        });
    }

    // -- Advanced -----------------------------------------------------------

    fn advanced(&mut self, ui: &mut egui::Ui, s: &Snapshot) {
        section(ui, "Courbe ventilateurs personnalisée");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label("La vitesse suit la température la plus haute (GPU, CPU, capteur IR). Appliquée toutes les secondes.");
            let mut remove = None;
            for (i, p) in self.curve_edit.points.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("Point {}", i + 1));
                    ui.add(egui::DragValue::new(&mut p.temp_c).range(20..=100).suffix(" °C"));
                    ui.label("CPU");
                    let mut cpu = u32::from(p.cpu_level) * 100;
                    if ui.add(egui::DragValue::new(&mut cpu).range(2000..=5900).speed(50).suffix(" tr/min")).changed() {
                        p.cpu_level = (cpu / 100) as u8;
                    }
                    ui.label("GPU");
                    let mut gpu = u32::from(p.gpu_level) * 100;
                    if ui.add(egui::DragValue::new(&mut gpu).range(2000..=5900).speed(50).suffix(" tr/min")).changed() {
                        p.gpu_level = (gpu / 100) as u8;
                    }
                    if ui.small_button("✖").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                if self.curve_edit.points.len() > 2 {
                    self.curve_edit.points.remove(i);
                }
            }
            ui.horizontal(|ui| {
                if ui.button("+ Point").clicked() {
                    let last = self.curve_edit.points.last().copied().unwrap_or(FanCurvePoint { temp_c: 40, cpu_level: 0x1A, gpu_level: 0x1A });
                    self.curve_edit.points.push(FanCurvePoint {
                        temp_c: last.temp_c.saturating_add(8).min(100),
                        cpu_level: last.cpu_level.saturating_add(4).min(FanCurve::LEVEL_MAX),
                        gpu_level: last.gpu_level.saturating_add(4).min(FanCurve::LEVEL_MAX),
                    });
                }
                if ui.button("Par défaut").clicked() {
                    self.curve_edit = FanCurve::default_curve();
                }
                let active = s.mode == Mode::Custom;
                let label = if active { "Mettre à jour la courbe" } else { "Utiliser cette courbe" };
                if ui.add(egui::Button::new(RichText::new(label).strong()).fill(ACCENT_DIM)).clicked() {
                    self.curve_edit.points.sort_by_key(|p| p.temp_c);
                    self.controller.send(Command::SetCustomCurve(self.curve_edit.clone()));
                    {
                        let mut cfg = self.cfg();
                        cfg.custom_curve = self.curve_edit.clone();
                        cfg.save();
                    }
                    if !active {
                        self.set_mode(Mode::Custom);
                    }
                }
            });
            let pts = |f: fn(&FanCurvePoint) -> u8| -> PlotPoints {
                self.curve_edit.points.iter().map(|p| [p.temp_c as f64, f(p) as f64 * 100.0]).collect()
            };
            Plot::new("curve")
                .legend(Legend::default())
                .height(150.0)
                .allow_scroll(false)
                .include_y(0.0)
                .show(ui, |p| {
                    p.line(Line::new("CPU tr/min", pts(|p| p.cpu_level)));
                    p.line(Line::new("GPU tr/min", pts(|p| p.gpu_level)));
                });
        });

        section(ui, "Vitesse fixe des ventilateurs");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.add(egui::Slider::new(&mut self.manual_rpm, 2000..=5900).step_by(100.0).suffix(" tr/min"));
                if ui.button("Fixer").clicked() {
                    self.controller.send(Command::SetManualFan(Some((self.manual_rpm / 100) as u8)));
                }
                if ui.add_enabled(s.manual_fan.is_some(), egui::Button::new("Rendre la main")).clicked() {
                    self.controller.send(Command::SetManualFan(None));
                }
            });
            ui.label(RichText::new("Pour tester ou forcer une vitesse ; reste actif jusqu'à « Rendre la main » ou la fermeture de l'application.").size(12.0).color(MUTED));
        });

        section(ui, "Carte graphique");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            if let Some(g) = s.gpu_power {
                let mut tgp = g.custom_tgp == 1;
                let mut ppab = g.ppab == 1;
                ui.horizontal(|ui| {
                    let a = ui.checkbox(&mut tgp, "Puissance GPU étendue (TGP)").changed();
                    let b = ui.checkbox(&mut ppab, "Dynamic Boost (PPAB)").changed();
                    if a || b {
                        self.controller.send(Command::SetGpuPower { custom_tgp: tgp, ppab });
                    }
                });
                ui.label(RichText::new("Les modes règlent déjà ces options (Silencieux : off, Équilibré : TGP, Performance/Max : TGP + Boost).").size(12.0).color(MUTED));
            }
            ui.horizontal(|ui| {
                let current = match s.gpu_mode {
                    Some(GpuMode::Hybrid) => "Hybride (Optimus, économie d'énergie)",
                    Some(GpuMode::Discrete) => "GPU dédié seul (performances max)",
                    Some(GpuMode::Optimus) => "Optimus",
                    Some(GpuMode::Unknown(_)) => "inconnu",
                    None => "—",
                };
                ui.label(format!("Mode graphique : {current}"));
                if ui.button("Hybride").clicked() {
                    self.confirm_mux = Some(GpuMode::Hybrid);
                }
                if ui.button("GPU dédié").clicked() {
                    self.confirm_mux = Some(GpuMode::Discrete);
                }
            });
            if let Some(m) = self.confirm_mux {
                ui.horizontal(|ui| {
                    ui.colored_label(WARN, format!("Passer en {m:?} ? Un redémarrage est nécessaire pour l'appliquer."));
                    if ui.button("Confirmer").clicked() {
                        self.controller.send(Command::SetGpuMux(m));
                        self.confirm_mux = None;
                    }
                    if ui.button("Annuler").clicked() {
                        self.confirm_mux = None;
                    }
                });
            }
        });

        section(ui, "Informations");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::Grid::new("info").num_columns(2).striped(true).show(ui, |ui| {
                let t = |v: Option<u8>| v.map(|x| format!("{x} °C")).unwrap_or_else(|| "—".into());
                let row = |ui: &mut egui::Ui, k: &str, v: String| {
                    ui.label(RichText::new(k).color(MUTED));
                    ui.label(v);
                    ui.end_row();
                };
                if let Some(sys) = &s.system {
                    row(ui, "Politique thermique BIOS", format!("V{}", sys.thermal_policy));
                    row(ui, "Contrôle logiciel des ventilateurs", if sys.sw_fan_control() { "supporté".into() } else { "non".into() });
                    row(ui, "PL4 par défaut", format!("{} W", sys.default_cpu_pl4));
                }
                row(ui, "Nombre de ventilateurs", s.fan_count.to_string());
                row(ui, "Capteur IR (châssis)", t(s.temp_ir));
                row(ui, "Capteur ambiant", t(s.temp_ambient));
                row(ui, "Capteur PCH", t(s.temp_pch));
                row(ui, "Capteur VR (alimentation CPU)", t(s.temp_vr));
                row(ui, "Zone thermique ACPI", fmt_opt(s.thermal_zone, " °C"));
                row(ui, "GPU", s.gpu_name.clone().unwrap_or_else(|| "—".into()));
                if let Some(a) = s.adapter {
                    row(ui, "Chargeur", format!("{} ({} W)", a.label(), a.watts));
                }
                if let Some(g) = s.gpu_power {
                    row(ui, "Seuil de température GPU", format!("{} °C", g.peak_temperature));
                }
                row(ui, "Bridage thermique (BIOS)", if s.throttling { "oui".into() } else { "non".into() });
            });
            if let Some(ft) = &s.fan_table {
                ui.collapsing(format!("Table ventilateurs du BIOS ({} paliers)", ft.level_count), |ui| {
                    for e in &ft.entries {
                        ui.label(format!(
                            "{} °C : CPU {} tr/min, GPU {} tr/min",
                            e.temp_c,
                            omen_wmi::level_to_rpm(e.cpu),
                            omen_wmi::level_to_rpm(e.gpu)
                        ));
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui.button("Relire les infos BIOS").clicked() {
                    self.controller.send(Command::RefreshInfo);
                }
                if ui.button("Ouvrir le dossier des journaux").clicked() {
                    let _ = std::process::Command::new("explorer.exe").arg(omen_core::config::data_dir()).spawn();
                }
            });
        });

        section(ui, "Bon à savoir");
        card(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label("• Les modes envoient au BIOS le mode thermique HP (0x1A), la puissance GPU et le mode ventilateurs max. Sur ce modèle, le mode thermique est accepté sans effet mesurable à vide : la différence se voit surtout sous charge.");
            ui.label("• Les températures CPU exactes (cœurs) ne sont pas lisibles sans pilote noyau ; l'application utilise la zone thermique ACPI, les capteurs du BIOS et la charge CPU.");
            ui.label("• Le registre « rétroéclairage » du BIOS ne pilote pas les LED de ce clavier : l'éclairage passe par le contrôleur USB du clavier, comme dans l'application HP.");
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let s = self.controller.snapshot();

        if let Some(tray) = &mut self.tray {
            let tip = match (s.mode, s.applied) {
                (Mode::Auto, Some(l)) => format!("{APP_TITLE} — Auto ({})", l.label()),
                (m, _) => format!("{APP_TITLE} — {}", m.label()),
            };
            tray.sync(s.mode, &tip);
        }

        if self.hide_on_start {
            self.hide_on_start = false;
            if self.tray.is_some() {
                crate::hide_main_window(&ctx);
            }
        }

        // Close button: keep running in the notification area.
        if ctx.input(|i| i.viewport().close_requested()) {
            let to_tray = self.cfg().close_to_tray && self.tray.is_some();
            if to_tray {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::hide_main_window(&ctx);
                if !self.quitting_hint_shown {
                    self.quitting_hint_shown = true;
                    crate::log_line("ui: hidden to tray");
                }
            } else {
                crate::quit(&self.controller, &self.config);
            }
        }

        egui::CentralPanel::default().show(ui, |ui| {
            self.header(ui, &s);
            self.tabs(ui);
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                match self.tab {
                    Tab::Home => self.home(ui, &s),
                    Tab::Keyboard => self.keyboard(ui, &s),
                    Tab::Settings => self.settings(ui, &s),
                    Tab::Advanced => self.advanced(ui, &s),
                }
                ui.add_space(12.0);
            });
        });
    }
}
