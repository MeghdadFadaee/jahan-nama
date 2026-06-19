use std::path::PathBuf;

use jahan_nama::Result;

#[cfg(windows)]
pub fn run_gui(env_path: PathBuf, interval_seconds: u64) -> Result<()> {
    windows_gui::run(env_path, interval_seconds)
}

#[cfg(target_os = "macos")]
pub fn run_gui(env_path: PathBuf, interval_seconds: u64) -> Result<()> {
    macos_gui::run(env_path, interval_seconds)
}

#[cfg(all(not(windows), not(target_os = "macos")))]
pub fn run_gui(_env_path: PathBuf, _interval_seconds: u64) -> Result<()> {
    Err(jahan_nama::JahanNamaError::Gui(
        "GUI mode is currently implemented for Windows and macOS only.".to_owned(),
    ))
}

#[cfg(target_os = "macos")]
mod macos_gui {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::{self, Command};
    use std::sync::mpsc::{self, Receiver, Sender};
    use std::thread;
    use std::time::{Duration, Instant};

    use eframe::egui::{
        self, Align, Button, CentralPanel, Color32, Context, CornerRadius, DragValue, Frame, Id,
        Key, Layout, Margin, Modifiers, RichText, ScrollArea, Sense, Stroke, TextEdit, Vec2,
        ViewportBuilder, ViewportCommand, ViewportId,
    };
    use jahan_nama::format::remaining_label;
    use jahan_nama::{DotEnvStore, JahanNamaClient, JahanNamaError, Result, reset_saved_token};
    use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{TrayIcon, TrayIconBuilder};

    const DEFAULT_LABEL_FONT: &str = "SF Pro Text";
    const DEFAULT_LABEL_FONT_SIZE: i32 = 15;
    const DEFAULT_OVERLAY_WIDTH: f32 = 300.0;
    const DEFAULT_OVERLAY_HEIGHT: f32 = 76.0;
    const DEFAULT_OVERLAY_X: f32 = 960.0;
    const DEFAULT_OVERLAY_Y: f32 = 64.0;
    const SETTINGS_BG: Color32 = Color32::from_rgb(244, 247, 251);
    const SETTINGS_SURFACE: Color32 = Color32::from_rgb(255, 255, 255);
    const SETTINGS_BORDER: Color32 = Color32::from_rgb(218, 225, 235);
    const SETTINGS_TEXT: Color32 = Color32::from_rgb(24, 33, 45);
    const SETTINGS_MUTED: Color32 = Color32::from_rgb(91, 103, 118);
    const SETTINGS_ACCENT: Color32 = Color32::from_rgb(14, 109, 242);

    const MENU_RELOAD: &str = "reload";
    const MENU_TOGGLE_OVERLAY: &str = "toggle-overlay";
    const MENU_SETTINGS: &str = "settings";
    const MENU_RESET_TOKEN: &str = "reset-token";
    const MENU_OPEN_CONFIG: &str = "open-config";
    const MENU_QUIT: &str = "quit";

    pub fn run(env_path: PathBuf, interval_seconds: u64) -> Result<()> {
        let env_path = resolve_env_path(env_path);
        let settings = SettingsValues::load(&env_path, interval_seconds);
        let viewport = ViewportBuilder::default()
            .with_title("Jahan Nama")
            .with_inner_size([DEFAULT_OVERLAY_WIDTH, DEFAULT_OVERLAY_HEIGHT])
            .with_min_inner_size([260.0, 76.0])
            .with_resizable(false)
            .with_decorations(false)
            .with_transparent(true)
            .with_has_shadow(false)
            .with_position(settings.overlay_position());

        let options = eframe::NativeOptions {
            viewport,
            ..Default::default()
        };

        eframe::run_native(
            "Jahan Nama",
            options,
            Box::new(move |creation| {
                Ok(Box::new(MacApp::new(
                    creation,
                    env_path.clone(),
                    settings.clone(),
                )))
            }),
        )
        .map_err(|error| JahanNamaError::Gui(error.to_string()))
    }

    #[derive(Clone)]
    struct SettingsValues {
        username: String,
        password: String,
        interval_seconds: u64,
        label_font: String,
        label_font_size: i32,
        overlay_visible: bool,
        overlay_x: Option<f32>,
        overlay_y: Option<f32>,
    }

    impl SettingsValues {
        fn load(env_path: &Path, interval_seconds: u64) -> Self {
            let env = DotEnvStore::new(env_path).ok();
            let get = |key: &str| {
                env.as_ref()
                    .and_then(|store| store.get(key))
                    .unwrap_or_default()
                    .to_owned()
            };

            Self {
                username: get("JAHAN_NAMA_USERNAME"),
                password: get("JAHAN_NAMA_PASSWORD"),
                interval_seconds: get("JAHAN_NAMA_INTERVAL_SECONDS")
                    .parse()
                    .ok()
                    .filter(|value| *value > 0)
                    .unwrap_or(interval_seconds.max(1)),
                label_font: {
                    let value = get("JAHAN_NAMA_LABEL_FONT_FAMILY");
                    if value.is_empty() {
                        DEFAULT_LABEL_FONT.to_owned()
                    } else {
                        value
                    }
                },
                label_font_size: get("JAHAN_NAMA_LABEL_FONT_SIZE")
                    .parse()
                    .ok()
                    .filter(|value| (9..=48).contains(value))
                    .unwrap_or(DEFAULT_LABEL_FONT_SIZE),
                overlay_visible: get_bool(&get("JAHAN_NAMA_OVERLAY_VISIBLE")).unwrap_or(true),
                overlay_x: get("JAHAN_NAMA_OVERLAY_X").parse().ok(),
                overlay_y: get("JAHAN_NAMA_OVERLAY_Y").parse().ok(),
            }
        }

        fn overlay_position(&self) -> egui::Pos2 {
            egui::pos2(
                self.overlay_x.unwrap_or(DEFAULT_OVERLAY_X),
                self.overlay_y.unwrap_or(DEFAULT_OVERLAY_Y),
            )
        }

        fn has_credentials(&self) -> bool {
            !self.username.trim().is_empty() && !self.password.is_empty()
        }
    }

    #[derive(Clone)]
    struct SettingsDraft {
        username: String,
        password: String,
        interval_seconds: u64,
        label_font: String,
        label_font_size: i32,
        overlay_visible: bool,
    }

    impl From<&SettingsValues> for SettingsDraft {
        fn from(values: &SettingsValues) -> Self {
            Self {
                username: values.username.clone(),
                password: values.password.clone(),
                interval_seconds: values.interval_seconds,
                label_font: values.label_font.clone(),
                label_font_size: values.label_font_size,
                overlay_visible: values.overlay_visible,
            }
        }
    }

    enum FetchMessage {
        Done(std::result::Result<f64, String>),
    }

    struct TrayHandles {
        tray: TrayIcon,
        _reload: MenuItem,
        toggle_overlay: MenuItem,
        _settings: MenuItem,
        _reset: MenuItem,
        _open_config: MenuItem,
        _quit: MenuItem,
        _separator_a: PredefinedMenuItem,
        _separator_b: PredefinedMenuItem,
    }

    struct MacApp {
        env_path: PathBuf,
        settings: SettingsValues,
        draft: SettingsDraft,
        label: String,
        detail: String,
        status: LabelStatus,
        fetching: bool,
        next_fetch: Instant,
        fetch_tx: Sender<FetchMessage>,
        fetch_rx: Receiver<FetchMessage>,
        menu_rx: Receiver<String>,
        tray: Option<TrayHandles>,
        show_settings: bool,
        message: Option<String>,
        applied_visibility: Option<bool>,
        quit_requested: bool,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum LabelStatus {
        Loading,
        Ready,
        Error,
    }

    impl MacApp {
        fn new(
            creation: &eframe::CreationContext<'_>,
            env_path: PathBuf,
            settings: SettingsValues,
        ) -> Self {
            let (fetch_tx, fetch_rx) = mpsc::channel();
            let (menu_tx, menu_rx) = mpsc::channel();
            let ctx = creation.egui_ctx.clone();
            let menu_env_path = env_path.clone();
            creation.egui_ctx.set_visuals(egui::Visuals::light());
            MenuEvent::set_event_handler(Some(move |event: MenuEvent| {
                let id = event.id().as_ref().to_owned();
                match id.as_str() {
                    MENU_QUIT => process::exit(0),
                    MENU_OPEN_CONFIG => {
                        open_config_folder_path(&menu_env_path);
                        return;
                    }
                    MENU_RESET_TOKEN => {
                        let _ = ensure_env_parent(&menu_env_path)
                            .and_then(|_| reset_saved_token(&menu_env_path));
                    }
                    MENU_TOGGLE_OVERLAY | MENU_SETTINGS => {
                        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
                    }
                    _ => {}
                }

                let _ = menu_tx.send(id);
                ctx.request_repaint();
            }));

            let mut app = Self {
                env_path,
                draft: SettingsDraft::from(&settings),
                settings,
                label: "Loading".to_owned(),
                detail: "Checking remaining traffic".to_owned(),
                status: LabelStatus::Loading,
                fetching: false,
                next_fetch: Instant::now(),
                fetch_tx,
                fetch_rx,
                menu_rx,
                tray: None,
                show_settings: false,
                message: None,
                applied_visibility: None,
                quit_requested: false,
            };

            app.tray = match create_tray(&app.label, app.settings.overlay_visible) {
                Ok(tray) => Some(tray),
                Err(error) => {
                    app.message = Some(format!("Menu bar item unavailable: {error}"));
                    None
                }
            };

            if !app.settings.has_credentials() {
                app.show_missing_credentials();
                app.open_settings(true);
                app.update_tray_title();
            } else {
                app.start_fetch(&creation.egui_ctx, true);
            }

            app
        }

        fn start_fetch(&mut self, ctx: &Context, force: bool) {
            if self.fetching && !force {
                return;
            }

            if !self.settings.has_credentials() {
                self.show_missing_credentials();
                if force && !self.show_settings {
                    self.open_settings(true);
                }
                self.next_fetch =
                    Instant::now() + Duration::from_secs(self.settings.interval_seconds.max(1));
                self.update_tray_title();
                return;
            }

            self.fetching = true;
            self.status = LabelStatus::Loading;
            self.detail = "Checking remaining traffic".to_owned();
            self.update_tray_title();

            let env_path = self.env_path.clone();
            let tx = self.fetch_tx.clone();
            let ctx = ctx.clone();
            thread::spawn(move || {
                let result = fetch_remaining_mb(&env_path).map_err(|error| error.to_string());
                let _ = tx.send(FetchMessage::Done(result));
                ctx.request_repaint();
            });
        }

        fn apply_fetch_result(&mut self, result: std::result::Result<f64, String>) {
            self.fetching = false;
            self.next_fetch =
                Instant::now() + Duration::from_secs(self.settings.interval_seconds.max(1));

            match result {
                Ok(megabytes) => {
                    self.status = LabelStatus::Ready;
                    self.label = remaining_label(megabytes);
                    self.detail = "Remaining traffic".to_owned();
                    self.message = None;
                }
                Err(error) => {
                    self.status = LabelStatus::Error;
                    self.label = "Refresh failed".to_owned();
                    self.detail = compact_error(&error);
                    self.message = Some(error);
                }
            }

            self.update_tray_title();
        }

        fn show_missing_credentials(&mut self) {
            self.status = LabelStatus::Error;
            self.label = "Setup needed".to_owned();
            self.detail = "Add your account in Settings".to_owned();
            self.message = Some("Enter your Jahan Nama username and password.".to_owned());
        }

        fn open_settings(&mut self, refresh_draft: bool) {
            if refresh_draft || !self.show_settings {
                self.draft = SettingsDraft::from(&self.settings);
            }
            self.show_settings = true;
        }

        fn save_settings(&mut self, ctx: &Context) {
            let result = (|| -> Result<()> {
                ensure_env_parent(&self.env_path)?;
                let mut env = DotEnvStore::new(&self.env_path)?;
                env.set("JAHAN_NAMA_USERNAME", self.draft.username.trim());
                env.set("JAHAN_NAMA_PASSWORD", self.draft.password.clone());
                env.set(
                    "JAHAN_NAMA_INTERVAL_SECONDS",
                    self.draft.interval_seconds.max(1).to_string(),
                );
                env.set(
                    "JAHAN_NAMA_LABEL_FONT_FAMILY",
                    self.draft.label_font.clone(),
                );
                env.set(
                    "JAHAN_NAMA_LABEL_FONT_SIZE",
                    self.draft.label_font_size.clamp(9, 48).to_string(),
                );
                env.set(
                    "JAHAN_NAMA_OVERLAY_VISIBLE",
                    bool_env(self.draft.overlay_visible),
                );
                env.save()
            })();

            match result {
                Ok(()) => {
                    self.settings.username = self.draft.username.trim().to_owned();
                    self.settings.password = self.draft.password.clone();
                    self.settings.interval_seconds = self.draft.interval_seconds.max(1);
                    self.settings.label_font = self.draft.label_font.clone();
                    self.settings.label_font_size = self.draft.label_font_size.clamp(9, 48);
                    self.settings.overlay_visible = self.draft.overlay_visible;
                    self.message = Some("Settings saved.".to_owned());
                    self.update_overlay_menu_text();
                    self.apply_overlay_visibility(ctx);
                    self.start_fetch(ctx, true);
                }
                Err(error) => {
                    self.message = Some(format!("Could not save settings: {error}"));
                }
            }
        }

        fn reset_token(&mut self, ctx: &Context) {
            let result =
                ensure_env_parent(&self.env_path).and_then(|_| reset_saved_token(&self.env_path));
            match result {
                Ok(()) => {
                    self.message = Some("Saved token cleared.".to_owned());
                    self.start_fetch(ctx, true);
                }
                Err(error) => {
                    self.message = Some(format!("Could not clear token: {error}"));
                }
            }
        }

        fn open_config_folder(&mut self) {
            match self.env_path.parent() {
                Some(parent) => {
                    let _ = fs::create_dir_all(parent);
                    if let Err(error) = Command::new("open").arg(parent).spawn() {
                        self.message = Some(format!("Could not open config folder: {error}"));
                    }
                }
                None => {
                    self.message = Some("Config folder is not available.".to_owned());
                }
            }
        }

        fn update_tray_title(&self) {
            if let Some(tray) = &self.tray {
                tray.tray.set_title(Some(self.label.as_str()));
                let tooltip = format!("Jahan Nama - {}", self.detail);
                let _ = tray.tray.set_tooltip(Some(tooltip.as_str()));
            }
        }

        fn update_overlay_menu_text(&self) {
            if let Some(tray) = &self.tray {
                let label = if self.settings.overlay_visible {
                    "Hide Overlay"
                } else {
                    "Show Overlay"
                };
                tray.toggle_overlay.set_text(label);
            }
        }

        fn apply_overlay_visibility(&mut self, ctx: &Context) {
            if self.applied_visibility != Some(self.settings.overlay_visible) {
                ctx.send_viewport_cmd(ViewportCommand::Visible(self.settings.overlay_visible));
                self.applied_visibility = Some(self.settings.overlay_visible);
            }
        }

        fn handle_menu(&mut self, id: &str, ctx: &Context) {
            match id {
                MENU_RELOAD => self.start_fetch(ctx, true),
                MENU_TOGGLE_OVERLAY => {
                    self.settings.overlay_visible = !self.settings.overlay_visible;
                    self.draft.overlay_visible = self.settings.overlay_visible;
                    self.update_overlay_menu_text();
                    self.apply_overlay_visibility(ctx);
                }
                MENU_SETTINGS => self.open_settings(false),
                MENU_RESET_TOKEN => self.reset_token(ctx),
                MENU_OPEN_CONFIG => self.open_config_folder(),
                MENU_QUIT => {
                    self.quit_requested = true;
                    ctx.send_viewport_cmd(ViewportCommand::Close);
                }
                _ => {}
            }
        }

        fn show_overlay(&mut self, ui: &mut egui::Ui, ctx: &Context) {
            let fill = match self.status {
                LabelStatus::Loading => Color32::from_rgb(31, 36, 45),
                LabelStatus::Ready => Color32::from_rgb(21, 42, 37),
                LabelStatus::Error => Color32::from_rgb(54, 28, 32),
            };
            let accent = match self.status {
                LabelStatus::Loading => Color32::from_rgb(152, 170, 192),
                LabelStatus::Ready => Color32::from_rgb(83, 210, 153),
                LabelStatus::Error => Color32::from_rgb(245, 112, 112),
            };

            ui.set_min_size(ui.available_size());
            Frame::new()
                .fill(fill)
                .stroke(Stroke::new(1.0, accent.linear_multiply(0.5)))
                .corner_radius(CornerRadius::same(10))
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    let drag_rect = ui.max_rect();
                    let drag_response =
                        ui.interact(drag_rect, Id::new("overlay-drag"), Sense::click_and_drag());
                    if drag_response.drag_started() {
                        ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                    }

                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new(&self.detail)
                                    .size(12.0)
                                    .color(Color32::from_rgb(184, 194, 207)),
                            );
                            ui.label(
                                RichText::new(&self.label)
                                    .size(self.settings.label_font_size as f32 + 7.0)
                                    .strong()
                                    .color(Color32::WHITE),
                            );
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let settings_button = Button::new(RichText::new("⚙").size(16.0))
                                .min_size(Vec2::splat(28.0));
                            if ui.add(settings_button).on_hover_text("Settings").clicked() {
                                self.open_settings(false);
                            }
                            if ui
                                .add_enabled(
                                    !self.fetching,
                                    Button::new(RichText::new("↻").size(17.0))
                                        .min_size(Vec2::splat(28.0)),
                                )
                                .on_hover_text("Reload now")
                                .clicked()
                            {
                                self.start_fetch(ctx, true);
                            }
                        });
                    });
                });
        }

        fn show_settings_window(&mut self, ctx: &Context) {
            let mut close_requested = false;
            let builder = ViewportBuilder::default()
                .with_title("Jahan Nama Settings")
                .with_inner_size([560.0, 460.0])
                .with_min_inner_size([520.0, 430.0])
                .with_resizable(false);

            ctx.show_viewport_immediate(
                ViewportId::from_hash_of("settings"),
                builder,
                |ui, _class| {
                    if ui.input(|input| input.viewport().close_requested()) {
                        close_requested = true;
                    }

                    CentralPanel::default()
                        .frame(
                            Frame::new()
                                .fill(SETTINGS_BG)
                                .inner_margin(Margin::same(18)),
                        )
                        .show_inside(ui, |ui| {
                            ui.visuals_mut().override_text_color = Some(SETTINGS_TEXT);
                            ui.spacing_mut().item_spacing = Vec2::new(10.0, 8.0);

                            ui.vertical(|ui| {
                                ui.label(
                                    RichText::new("Jahan Nama")
                                        .size(24.0)
                                        .strong()
                                        .color(SETTINGS_TEXT),
                                );
                                ui.label(
                                    RichText::new("Account and display settings")
                                        .size(13.0)
                                        .color(SETTINGS_MUTED),
                                );
                                ui.add_space(12.0);

                                Frame::new()
                                    .fill(SETTINGS_SURFACE)
                                    .stroke(Stroke::new(1.0, SETTINGS_BORDER))
                                    .corner_radius(CornerRadius::same(8))
                                    .inner_margin(Margin::same(14))
                                    .show(ui, |ui| {
                                        ScrollArea::vertical()
                                            .max_height(240.0)
                                            .auto_shrink([false, true])
                                            .show(ui, |ui| {
                                                settings_label(ui, "Username");
                                                ui.add(
                                                    TextEdit::singleline(&mut self.draft.username)
                                                        .desired_width(f32::INFINITY),
                                                );
                                                ui.add_space(8.0);

                                                settings_label(ui, "Password");
                                                ui.add(
                                                    TextEdit::singleline(&mut self.draft.password)
                                                        .password(true)
                                                        .desired_width(f32::INFINITY),
                                                );
                                                ui.add_space(8.0);

                                                ui.horizontal(|ui| {
                                                    settings_label(ui, "Refresh interval");
                                                    ui.add(
                                                        DragValue::new(
                                                            &mut self.draft.interval_seconds,
                                                        )
                                                        .range(5..=86_400)
                                                        .speed(1.0),
                                                    );
                                                    ui.label(
                                                        RichText::new("seconds")
                                                            .color(SETTINGS_MUTED),
                                                    );
                                                });

                                                ui.horizontal(|ui| {
                                                    settings_label(ui, "Font");
                                                    ui.add(
                                                        TextEdit::singleline(
                                                            &mut self.draft.label_font,
                                                        )
                                                        .desired_width(210.0),
                                                    );
                                                    settings_label(ui, "Size");
                                                    ui.add(
                                                        DragValue::new(
                                                            &mut self.draft.label_font_size,
                                                        )
                                                        .range(9..=48)
                                                        .speed(1.0),
                                                    );
                                                });

                                                ui.checkbox(
                                                    &mut self.draft.overlay_visible,
                                                    "Show floating overlay",
                                                );
                                            });
                                    });

                                if let Some(message) = &self.message {
                                    ui.add_space(8.0);
                                    ui.label(
                                        RichText::new(message).size(13.0).color(SETTINGS_MUTED),
                                    );
                                }

                                ui.add_space(8.0);
                                ui.separator();
                                ui.add_space(8.0);

                                ui.horizontal_wrapped(|ui| {
                                    if ui.add_sized([72.0, 32.0], Button::new("Close")).clicked() {
                                        close_requested = true;
                                    }
                                    if ui.add_sized([82.0, 32.0], Button::new("Reload")).clicked() {
                                        self.start_fetch(ctx, true);
                                    }
                                    if ui.add_sized([92.0, 32.0], Button::new("Reset")).clicked() {
                                        self.reset_token(ctx);
                                    }
                                    if ui.add_sized([86.0, 32.0], Button::new("Open")).clicked() {
                                        self.open_config_folder();
                                    }
                                    if ui
                                        .add_sized(
                                            [86.0, 32.0],
                                            Button::new(
                                                RichText::new("Save")
                                                    .strong()
                                                    .color(Color32::WHITE),
                                            )
                                            .fill(SETTINGS_ACCENT),
                                        )
                                        .clicked()
                                    {
                                        self.save_settings(ctx);
                                    }
                                });
                            });
                        });
                },
            );

            if close_requested {
                self.show_settings = false;
            }
        }
    }

    impl eframe::App for MacApp {
        fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
            let ctx = ui.ctx().clone();

            while let Ok(message) = self.fetch_rx.try_recv() {
                match message {
                    FetchMessage::Done(result) => self.apply_fetch_result(result),
                }
            }

            while let Ok(id) = self.menu_rx.try_recv() {
                self.handle_menu(&id, &ctx);
            }

            if self.quit_requested {
                process::exit(0);
            }

            if ui.input(|input| {
                input.key_pressed(Key::Q) && input.modifiers.matches_logically(Modifiers::COMMAND)
            }) {
                process::exit(0);
            }

            self.apply_overlay_visibility(&ctx);

            if self.settings.has_credentials()
                && !self.fetching
                && Instant::now() >= self.next_fetch
            {
                self.start_fetch(&ctx, false);
            }

            self.show_overlay(ui, &ctx);

            if self.show_settings {
                self.show_settings_window(&ctx);
            }

            ctx.request_repaint_after(Duration::from_millis(250));
        }

        fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
            [0.0, 0.0, 0.0, 0.0]
        }
    }

    fn settings_label(ui: &mut egui::Ui, text: &str) {
        ui.label(RichText::new(text).size(13.0).strong().color(SETTINGS_TEXT));
    }

    fn fetch_remaining_mb(env_path: &Path) -> Result<f64> {
        let mut client = JahanNamaClient::new(env_path)?;
        client.get_remaining_traffic_mb()
    }

    fn create_tray(label: &str, overlay_visible: bool) -> std::result::Result<TrayHandles, String> {
        let menu = Menu::new();
        let reload = MenuItem::with_id(MENU_RELOAD, "Reload Now", true, None);
        let toggle_overlay = MenuItem::with_id(
            MENU_TOGGLE_OVERLAY,
            if overlay_visible {
                "Hide Overlay"
            } else {
                "Show Overlay"
            },
            true,
            None,
        );
        let settings = MenuItem::with_id(MENU_SETTINGS, "Settings", true, None);
        let reset = MenuItem::with_id(MENU_RESET_TOKEN, "Reset Token", true, None);
        let open_config = MenuItem::with_id(MENU_OPEN_CONFIG, "Open Config Folder", true, None);
        let quit = MenuItem::with_id(MENU_QUIT, "Quit", true, None);
        let separator_a = PredefinedMenuItem::separator();
        let separator_b = PredefinedMenuItem::separator();

        menu.append(&reload).map_err(|error| error.to_string())?;
        menu.append(&toggle_overlay)
            .map_err(|error| error.to_string())?;
        menu.append(&settings).map_err(|error| error.to_string())?;
        menu.append(&separator_a)
            .map_err(|error| error.to_string())?;
        menu.append(&reset).map_err(|error| error.to_string())?;
        menu.append(&open_config)
            .map_err(|error| error.to_string())?;
        menu.append(&separator_b)
            .map_err(|error| error.to_string())?;
        menu.append(&quit).map_err(|error| error.to_string())?;

        let tray = TrayIconBuilder::new()
            .with_title(label)
            .with_tooltip("Jahan Nama")
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(true)
            .build()
            .map_err(|error| error.to_string())?;

        Ok(TrayHandles {
            tray,
            _reload: reload,
            toggle_overlay,
            _settings: settings,
            _reset: reset,
            _open_config: open_config,
            _quit: quit,
            _separator_a: separator_a,
            _separator_b: separator_b,
        })
    }

    fn resolve_env_path(path: PathBuf) -> PathBuf {
        if path != PathBuf::from(".env") {
            return path;
        }

        if path.exists() || Path::new("Cargo.toml").exists() {
            return path;
        }

        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Jahan Nama")
            .join(".env")
    }

    fn ensure_env_parent(path: &Path) -> Result<()> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)?;
        }
        Ok(())
    }

    fn open_config_folder_path(env_path: &Path) {
        if let Some(parent) = env_path.parent() {
            let _ = fs::create_dir_all(parent);
            let _ = Command::new("open").arg(parent).spawn();
        }
    }

    fn get_bool(value: &str) -> Option<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Some(true),
            "0" | "false" | "no" | "off" => Some(false),
            _ => None,
        }
    }

    fn bool_env(value: bool) -> &'static str {
        if value { "true" } else { "false" }
    }

    fn compact_error(error: &str) -> String {
        if error.contains("Missing required environment variable") {
            "Account settings are incomplete".to_owned()
        } else if error.len() > 80 {
            format!("{}...", error.chars().take(77).collect::<String>())
        } else {
            error.to_owned()
        }
    }
}

#[cfg(windows)]
mod windows_gui {
    use std::ffi::{OsStr, OsString};
    use std::mem::{size_of, zeroed};
    use std::os::windows::ffi::{OsStrExt, OsStringExt};
    use std::path::PathBuf;
    use std::ptr::{null, null_mut};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use jahan_nama::format::remaining_label;
    use jahan_nama::{DotEnvStore, JahanNamaClient, JahanNamaError, Result, reset_saved_token};
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
    use windows_sys::Win32::Graphics::Gdi::{
        BeginPaint, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, CreateFontW, CreatePen,
        CreateSolidBrush, DEFAULT_CHARSET, DEFAULT_PITCH, DT_CENTER, DT_LEFT, DT_NOPREFIX,
        DT_SINGLELINE, DT_TOP, DT_VCENTER, DT_WORDBREAK, DeleteObject, DrawTextW, EndPaint,
        FF_DONTCARE, FW_NORMAL, FW_SEMIBOLD, FillRect, HBRUSH, HDC, HFONT, InvalidateRect,
        OUT_DEFAULT_PRECIS, PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject, SetBkColor, SetBkMode,
        SetTextColor, TRANSPARENT, UpdateWindow,
    };
    use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows_sys::Win32::UI::Controls::{
        DRAWITEMSTRUCT, ODS_DISABLED, ODS_HOTLIGHT, ODS_SELECTED,
    };
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
        ReleaseCapture, TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
    };
    use windows_sys::Win32::UI::Shell::{
        NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_SETVERSION, NOTIFYICON_VERSION_4,
        NOTIFYICONDATAW, Shell_NotifyIconW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, BS_OWNERDRAW, CREATESTRUCTW, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
        DestroyIcon, DestroyMenu, DestroyWindow, DispatchMessageW, ES_AUTOHSCROLL, ES_LEFT,
        ES_NUMBER, ES_PASSWORD, GWLP_USERDATA, GetClientRect, GetCursorPos, GetMessageW,
        GetSystemMetrics, GetWindowLongPtrW, GetWindowTextW, HICON, HMENU, HTCAPTION, ICON_BIG,
        ICON_SMALL, IDC_ARROW, IDI_APPLICATION, IMAGE_ICON, IsWindowVisible, KillTimer,
        LR_DEFAULTSIZE, LR_LOADFROMFILE, LWA_COLORKEY, LoadCursorW, LoadIconW, LoadImageW,
        MF_SEPARATOR, MF_STRING, MSG, PostMessageW, PostQuitMessage, RegisterClassW, SM_CXICON,
        SM_CYICON, SW_SHOW, SWP_NOACTIVATE, SWP_NOMOVE, SendMessageW, SetForegroundWindow,
        SetLayeredWindowAttributes, SetTimer, SetWindowLongPtrW, SetWindowPos, ShowWindow,
        TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, WM_APP, WM_CLOSE,
        WM_COMMAND, WM_CONTEXTMENU, WM_CTLCOLORDLG, WM_CTLCOLOREDIT, WM_CTLCOLORSTATIC, WM_DESTROY,
        WM_DRAWITEM, WM_ERASEBKGND, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_MOUSEMOVE, WM_NCCREATE,
        WM_NCDESTROY, WM_NCLBUTTONDOWN, WM_NULL, WM_PAINT, WM_RBUTTONDOWN, WM_RBUTTONUP,
        WM_SETFONT, WM_SETICON, WM_TIMER, WNDCLASSW, WS_CHILD, WS_CLIPCHILDREN, WS_EX_LAYERED,
        WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP, WS_TABSTOP, WS_VISIBLE,
    };

    const CLASS_NAME: &str = "JahanNamaOverlayWindow";
    const SETTINGS_CLASS_NAME: &str = "JahanNamaSettingsWindow";
    const WINDOW_WIDTH: i32 = 520;
    const MIN_WINDOW_HEIGHT: i32 = 36;
    const MAX_WINDOW_HEIGHT: i32 = 360;
    const SETTINGS_WIDTH: i32 = 620;
    const SETTINGS_HEIGHT: i32 = 510;
    const DEFAULT_LABEL_FONT: &str = "IRANSansWeb";
    const DEFAULT_LABEL_FONT_SIZE: i32 = 14;
    const UI_FONT: &str = "Segoe UI";
    const SETTINGS_BG: u32 = rgb(246, 248, 251);
    const SETTINGS_CARD: u32 = rgb(255, 255, 255);
    const SETTINGS_BORDER: u32 = rgb(222, 228, 236);
    const SETTINGS_TEXT: u32 = rgb(22, 28, 36);
    const SETTINGS_MUTED: u32 = rgb(91, 103, 118);
    const SETTINGS_ACCENT: u32 = rgb(20, 111, 237);
    const SETTINGS_EDIT_BG: u32 = rgb(255, 255, 255);
    const SETTINGS_INPUT_BORDER: u32 = rgb(205, 215, 228);
    const SETTINGS_INPUT_BG: u32 = rgb(252, 253, 255);
    const TIMER_ID: usize = 1;
    const TRAY_ID: u32 = 1;
    const WM_FETCH_DONE: u32 = WM_APP + 1;
    const WM_TRAY_ICON: u32 = WM_APP + 2;
    const WM_MOUSELEAVE: u32 = 0x02A3;

    const MENU_MANAGE: usize = 1001;
    const MENU_RELOAD: usize = 1002;
    const MENU_RESET: usize = 1003;
    const MENU_QUIT: usize = 1004;

    const SETTINGS_USERNAME: usize = 2001;
    const SETTINGS_PASSWORD: usize = 2002;
    const SETTINGS_INTERVAL: usize = 2003;
    const SETTINGS_FONT: usize = 2004;
    const SETTINGS_FONT_SIZE: usize = 2005;
    const SETTINGS_SAVE: usize = 2010;
    const SETTINGS_CLEAR_TOKEN: usize = 2011;
    const SETTINGS_RELOAD: usize = 2012;
    const SETTINGS_CLOSE: usize = 2013;

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FetchAction {
        Reload,
        Reset,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum LabelKind {
        Normal,
        Busy,
        Error,
    }

    struct LabelUpdate {
        text: String,
        kind: LabelKind,
    }

    struct ClientSlot {
        env_path: PathBuf,
        client: Option<JahanNamaClient>,
    }

    struct SettingsValues {
        username: String,
        password: String,
        interval_seconds: u64,
        label_font: String,
        label_font_size: i32,
    }

    struct LoadedIcon {
        handle: HICON,
        owned: bool,
    }

    struct AppState {
        client: Arc<Mutex<ClientSlot>>,
        label: String,
        label_kind: LabelKind,
        fetching: bool,
        hover: bool,
        font: HFONT,
        click_count: u8,
        last_click: Option<Instant>,
        icon: HICON,
        icon_owned: bool,
        tray_added: bool,
        settings_hwnd: HWND,
        interval_seconds: u64,
        label_font: String,
        label_font_size: i32,
    }

    struct SettingsState {
        parent_hwnd: HWND,
        client: Arc<Mutex<ClientSlot>>,
        username: HWND,
        password: HWND,
        interval: HWND,
        font: HWND,
        font_size: HWND,
        title_font: HFONT,
        section_font: HFONT,
        label_font: HFONT,
        body_font: HFONT,
        background_brush: HBRUSH,
        edit_brush: HBRUSH,
    }

    struct ButtonVisual {
        label: &'static str,
        fill: u32,
        border: u32,
        text: u32,
        hot_fill: u32,
        pressed_fill: u32,
    }

    #[derive(Clone, Copy)]
    struct Bounds {
        x: i32,
        y: i32,
        width: i32,
        height: i32,
    }

    pub fn run(env_path: PathBuf, interval_seconds: u64) -> Result<()> {
        let initial_settings = load_settings_values(&env_path, interval_seconds);
        let client = Arc::new(Mutex::new(ClientSlot {
            env_path: env_path.clone(),
            client: None,
        }));
        let class_name = wide(CLASS_NAME);
        let title = wide("Jahan Nama");
        let interval_ms = initial_settings
            .interval_seconds
            .saturating_mul(1000)
            .clamp(250, u32::MAX as u64) as u32;

        unsafe {
            let instance = GetModuleHandleW(null());
            if instance.is_null() {
                return Err(JahanNamaError::Gui("GetModuleHandleW failed".to_owned()));
            }

            let icon = load_app_icon();
            let wnd_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: icon.handle,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class_name.as_ptr(),
            };

            if RegisterClassW(&wnd_class) == 0 {
                destroy_loaded_icon(icon);
                return Err(JahanNamaError::Gui("RegisterClassW failed".to_owned()));
            }

            let font = create_font(
                &initial_settings.label_font,
                initial_settings.label_font_size,
            );
            let state = Box::new(AppState {
                client,
                label: "Loading...".to_owned(),
                label_kind: LabelKind::Busy,
                fetching: false,
                hover: false,
                font,
                click_count: 0,
                last_click: None,
                icon: icon.handle,
                icon_owned: icon.owned,
                tray_added: false,
                settings_hwnd: null_mut(),
                interval_seconds: initial_settings.interval_seconds,
                label_font: initial_settings.label_font,
                label_font_size: initial_settings.label_font_size,
            });
            let state_ptr = Box::into_raw(state);
            let screen_width = GetSystemMetrics(0);

            let hwnd = CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOPMOST | WS_EX_TOOLWINDOW,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_VISIBLE,
                screen_width - WINDOW_WIDTH,
                0,
                WINDOW_WIDTH,
                MIN_WINDOW_HEIGHT,
                null_mut(),
                null_mut(),
                instance,
                state_ptr.cast(),
            );

            if hwnd.is_null() {
                drop(Box::from_raw(state_ptr));
                return Err(JahanNamaError::Gui("CreateWindowExW failed".to_owned()));
            }

            SetLayeredWindowAttributes(hwnd, rgb(0, 0, 0), 0, LWA_COLORKEY);
            if !icon.handle.is_null() {
                SendMessageW(hwnd, WM_SETICON, ICON_BIG as WPARAM, icon.handle as LPARAM);
                SendMessageW(
                    hwnd,
                    WM_SETICON,
                    ICON_SMALL as WPARAM,
                    icon.handle as LPARAM,
                );
            }
            ShowWindow(hwnd, SW_SHOW);
            UpdateWindow(hwnd);

            if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                state.tray_added = add_tray_icon(hwnd, state.icon);
                start_action(hwnd, state, FetchAction::Reload, Some("Loading..."));
            }
            SetTimer(hwnd, TIMER_ID, interval_ms, None);

            let mut message: MSG = zeroed();
            while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }

        Ok(())
    }

    unsafe extern "system" fn window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_NCCREATE => {
                    let create_struct = lparam as *const CREATESTRUCTW;
                    let state = (*create_struct).lpCreateParams as *mut AppState;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
                    1
                }
                WM_TIMER if wparam == TIMER_ID => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut()
                        && !state.fetching
                    {
                        start_action(hwnd, state, FetchAction::Reload, None);
                    }
                    0
                }
                WM_FETCH_DONE => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                        state.fetching = false;
                        if lparam != 0 {
                            let update = Box::from_raw(lparam as *mut LabelUpdate);
                            set_label(hwnd, state, update.text, update.kind);
                        }
                    }
                    0
                }
                WM_TRAY_ICON => {
                    match loword(lparam as usize) as u32 {
                        WM_RBUTTONUP | WM_RBUTTONDOWN | WM_CONTEXTMENU => show_tray_menu(hwnd),
                        WM_LBUTTONDBLCLK => manage_app(hwnd),
                        _ => {}
                    }
                    0
                }
                WM_LBUTTONDOWN => {
                    ReleaseCapture();
                    SendMessageW(hwnd, WM_NCLBUTTONDOWN, HTCAPTION as WPARAM, 0);
                    0
                }
                WM_RBUTTONDOWN => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                        let now = Instant::now();
                        state.click_count = if state
                            .last_click
                            .is_some_and(|last| now.duration_since(last) < Duration::from_secs(2))
                        {
                            state.click_count.saturating_add(1)
                        } else {
                            1
                        };
                        state.last_click = Some(now);

                        if state.click_count >= 3 {
                            DestroyWindow(hwnd);
                        }
                    }
                    0
                }
                WM_MOUSEMOVE => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut()
                        && !state.hover
                    {
                        state.hover = true;
                        let mut event = TRACKMOUSEEVENT {
                            cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        TrackMouseEvent(&mut event);
                        InvalidateRect(hwnd, null(), 1);
                    }
                    0
                }
                WM_MOUSELEAVE => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                        state.hover = false;
                        InvalidateRect(hwnd, null(), 1);
                    }
                    0
                }
                WM_PAINT => {
                    paint(hwnd);
                    0
                }
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    0
                }
                WM_DESTROY => {
                    KillTimer(hwnd, TIMER_ID);
                    0
                }
                WM_NCDESTROY => {
                    let state = state_from_hwnd(hwnd);
                    if !state.is_null() {
                        let state = Box::from_raw(state);
                        if !state.settings_hwnd.is_null() {
                            DestroyWindow(state.settings_hwnd);
                        }
                        if state.tray_added {
                            delete_tray_icon(hwnd);
                        }
                        if !state.font.is_null() {
                            DeleteObject(state.font);
                        }
                        if state.icon_owned && !state.icon.is_null() {
                            DestroyIcon(state.icon);
                        }
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    }
                    PostQuitMessage(0);
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
    }

    unsafe fn paint(hwnd: HWND) {
        unsafe {
            let mut paint: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut paint);
            let mut rect: RECT = zeroed();
            GetClientRect(hwnd, &mut rect);

            let brush = CreateSolidBrush(rgb(0, 0, 0));
            FillRect(hdc, &rect, brush);
            DeleteObject(brush);

            if let Some(state) = state_from_hwnd(hwnd).as_ref() {
                let old_font = SelectObject(hdc, state.font);
                SetBkMode(hdc, TRANSPARENT as i32);
                SetTextColor(hdc, text_color(state.label_kind, state.hover));

                let line_count = state.label.lines().count();
                let mut text_rect = rect;
                if line_count > 1 {
                    text_rect.top = 6;
                    text_rect.bottom -= 4;
                }

                let text = wide(&state.label);
                let flags = if line_count > 1 {
                    DT_CENTER | DT_WORDBREAK | DT_NOPREFIX
                } else {
                    DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX
                };
                DrawTextW(hdc, text.as_ptr(), -1, &mut text_rect, flags);

                if !old_font.is_null() {
                    SelectObject(hdc, old_font);
                }
            }

            EndPaint(hwnd, &paint);
        }
    }

    unsafe fn start_action(
        hwnd: HWND,
        state: &mut AppState,
        action: FetchAction,
        busy_text: Option<&str>,
    ) {
        if state.fetching {
            if busy_text.is_some() {
                unsafe {
                    set_label(hwnd, state, "Busy...".to_owned(), LabelKind::Busy);
                }
            }
            return;
        }

        state.fetching = true;
        if let Some(text) = busy_text {
            unsafe {
                set_label(hwnd, state, text.to_owned(), LabelKind::Busy);
            }
        }
        spawn_fetch(hwnd, Arc::clone(&state.client), action);
    }

    fn spawn_fetch(hwnd: HWND, client: Arc<Mutex<ClientSlot>>, action: FetchAction) {
        let hwnd_value = hwnd as isize;
        thread::spawn(move || {
            let update = match client.lock() {
                Ok(mut client) => client.run_action(action),
                Err(_) => LabelUpdate {
                    text: wrap_label_text("Error: client lock poisoned"),
                    kind: LabelKind::Error,
                },
            };

            let payload = Box::into_raw(Box::new(update));
            let posted =
                unsafe { PostMessageW(hwnd_value as HWND, WM_FETCH_DONE, 0, payload as LPARAM) };
            if posted == 0 {
                unsafe {
                    drop(Box::from_raw(payload));
                }
            }
        });
    }

    impl ClientSlot {
        fn run_action(&mut self, action: FetchAction) -> LabelUpdate {
            match action {
                FetchAction::Reload => self.reload_update(false),
                FetchAction::Reset => match self.reset_auth() {
                    Ok(()) => self.reload_update(true),
                    Err(error) => error_update(format!("Reset failed: {error}")),
                },
            }
        }

        fn reload_update(&mut self, did_reset: bool) -> LabelUpdate {
            match self.remaining_traffic_mb() {
                Ok(megabytes) => value_update(megabytes, did_reset),
                Err(error) => error_update(format!("Error: {error}")),
            }
        }

        fn reset_auth(&mut self) -> std::result::Result<(), String> {
            match self.client.as_mut() {
                Some(client) => client.reset_auth().map_err(|error| error.to_string())?,
                None => reset_saved_token(&self.env_path).map_err(|error| error.to_string())?,
            }
            self.client = None;
            Ok(())
        }

        fn remaining_traffic_mb(&mut self) -> std::result::Result<f64, String> {
            if self.client.is_none() {
                self.client =
                    Some(JahanNamaClient::new(&self.env_path).map_err(|error| error.to_string())?);
            }

            self.client
                .as_mut()
                .expect("client initialized above")
                .get_remaining_traffic_mb()
                .map_err(|error| error.to_string())
        }
    }

    fn value_update(megabytes: f64, did_reset: bool) -> LabelUpdate {
        let text = remaining_label(megabytes);

        LabelUpdate {
            text: if did_reset {
                format!("Token reset\n{text}")
            } else {
                text
            },
            kind: LabelKind::Normal,
        }
    }

    fn error_update(text: String) -> LabelUpdate {
        LabelUpdate {
            text: wrap_label_text(&text),
            kind: LabelKind::Error,
        }
    }

    unsafe fn show_tray_menu(hwnd: HWND) {
        unsafe {
            let menu = CreatePopupMenu();
            if menu.is_null() {
                if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                    set_label(
                        hwnd,
                        state,
                        "Error: failed to create tray menu".to_owned(),
                        LabelKind::Error,
                    );
                }
                return;
            }

            append_menu_item(menu, MENU_MANAGE, "Manage App");
            append_menu_item(menu, MENU_RELOAD, "Reload");
            append_menu_item(menu, MENU_RESET, "Reset Token");
            AppendMenuW(menu, MF_SEPARATOR, 0, null());
            append_menu_item(menu, MENU_QUIT, "Quit");

            let mut point: POINT = zeroed();
            if GetCursorPos(&mut point) == 0 {
                point.x = 0;
                point.y = 0;
            }

            SetForegroundWindow(hwnd);
            let command = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                0,
                hwnd,
                null(),
            );
            DestroyMenu(menu);
            PostMessageW(hwnd, WM_NULL, 0, 0);

            if command != 0 {
                handle_tray_command(hwnd, command as usize);
            }
        }
    }

    unsafe fn handle_tray_command(hwnd: HWND, command: usize) {
        unsafe {
            match command {
                MENU_MANAGE => manage_app(hwnd),
                MENU_RELOAD => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                        start_action(hwnd, state, FetchAction::Reload, Some("Reloading..."));
                    }
                }
                MENU_RESET => {
                    if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                        start_action(hwnd, state, FetchAction::Reset, Some("Resetting token..."));
                    }
                }
                MENU_QUIT => {
                    DestroyWindow(hwnd);
                }
                _ => {}
            }
        }
    }

    unsafe fn manage_app(hwnd: HWND) {
        unsafe {
            if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                if state.settings_hwnd.is_null() || IsWindowVisible(state.settings_hwnd) == 0 {
                    match create_settings_window(hwnd, state) {
                        Ok(settings_hwnd) => state.settings_hwnd = settings_hwnd,
                        Err(error) => {
                            set_label(hwnd, state, format!("Error: {error}"), LabelKind::Error);
                            return;
                        }
                    }
                }

                ShowWindow(state.settings_hwnd, SW_SHOW);
                SetForegroundWindow(state.settings_hwnd);
            }
        }
    }

    unsafe fn create_settings_window(hwnd: HWND, state: &mut AppState) -> Result<HWND> {
        unsafe {
            let instance = GetModuleHandleW(null());
            if instance.is_null() {
                return Err(JahanNamaError::Gui("GetModuleHandleW failed".to_owned()));
            }

            let class_name = wide(SETTINGS_CLASS_NAME);
            let wnd_class = WNDCLASSW {
                style: 0,
                lpfnWndProc: Some(settings_window_proc),
                cbClsExtra: 0,
                cbWndExtra: 0,
                hInstance: instance,
                hIcon: state.icon,
                hCursor: LoadCursorW(null_mut(), IDC_ARROW),
                hbrBackground: null_mut(),
                lpszMenuName: null(),
                lpszClassName: class_name.as_ptr(),
            };
            RegisterClassW(&wnd_class);

            let env_path = state
                .client
                .lock()
                .map_err(|_| JahanNamaError::Gui("client lock poisoned".to_owned()))?
                .env_path
                .clone();
            let settings = load_settings_values(&env_path, state.interval_seconds);
            let title = wide("Manage Jahan Nama");
            let settings_state = Box::new(SettingsState {
                parent_hwnd: hwnd,
                client: Arc::clone(&state.client),
                username: null_mut(),
                password: null_mut(),
                interval: null_mut(),
                font: null_mut(),
                font_size: null_mut(),
                title_font: create_weighted_font(UI_FONT, 21, FW_SEMIBOLD),
                section_font: create_weighted_font(UI_FONT, 11, FW_SEMIBOLD),
                label_font: create_weighted_font(UI_FONT, 9, FW_NORMAL),
                body_font: create_weighted_font(UI_FONT, 10, FW_NORMAL),
                background_brush: CreateSolidBrush(SETTINGS_BG),
                edit_brush: CreateSolidBrush(SETTINGS_EDIT_BG),
            });
            let state_ptr = Box::into_raw(settings_state);

            let settings_hwnd = CreateWindowExW(
                0,
                class_name.as_ptr(),
                title.as_ptr(),
                WS_POPUP | WS_CLIPCHILDREN | WS_VISIBLE,
                180,
                120,
                SETTINGS_WIDTH,
                SETTINGS_HEIGHT,
                hwnd,
                null_mut(),
                instance,
                state_ptr.cast(),
            );

            if settings_hwnd.is_null() {
                drop(Box::from_raw(state_ptr));
                return Err(JahanNamaError::Gui(
                    "Could not open Manage App window".to_owned(),
                ));
            }

            if !state.icon.is_null() {
                SendMessageW(
                    settings_hwnd,
                    WM_SETICON,
                    ICON_BIG as WPARAM,
                    state.icon as LPARAM,
                );
                SendMessageW(
                    settings_hwnd,
                    WM_SETICON,
                    ICON_SMALL as WPARAM,
                    state.icon as LPARAM,
                );
            }

            create_settings_controls(settings_hwnd, &settings);
            Ok(settings_hwnd)
        }
    }

    unsafe extern "system" fn settings_window_proc(
        hwnd: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> LRESULT {
        unsafe {
            match message {
                WM_NCCREATE => {
                    let create_struct = lparam as *const CREATESTRUCTW;
                    let state = (*create_struct).lpCreateParams as *mut SettingsState;
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, state as isize);
                    1
                }
                WM_ERASEBKGND => 1,
                WM_PAINT => {
                    paint_settings_window(hwnd);
                    0
                }
                WM_CTLCOLORDLG => settings_brush(hwnd, wparam as HDC, false),
                WM_CTLCOLORSTATIC => settings_brush(hwnd, wparam as HDC, false),
                WM_CTLCOLOREDIT => settings_brush(hwnd, wparam as HDC, true),
                WM_DRAWITEM => {
                    draw_owner_button(hwnd, lparam as *const DRAWITEMSTRUCT);
                    1
                }
                WM_COMMAND => {
                    let command = loword(wparam);
                    match command {
                        SETTINGS_SAVE => save_settings_from_window(hwnd),
                        SETTINGS_CLEAR_TOKEN => clear_token_from_window(hwnd),
                        SETTINGS_RELOAD => reload_from_settings_window(hwnd),
                        SETTINGS_CLOSE => {
                            DestroyWindow(hwnd);
                        }
                        _ => {}
                    }
                    0
                }
                WM_CLOSE => {
                    DestroyWindow(hwnd);
                    0
                }
                WM_NCDESTROY => {
                    let settings = settings_state_from_hwnd(hwnd);
                    if !settings.is_null() {
                        let settings = Box::from_raw(settings);
                        if let Some(parent_state) = state_from_hwnd(settings.parent_hwnd).as_mut() {
                            parent_state.settings_hwnd = null_mut();
                        }
                        delete_object(settings.title_font);
                        delete_object(settings.section_font);
                        delete_object(settings.label_font);
                        delete_object(settings.body_font);
                        delete_object(settings.background_brush);
                        delete_object(settings.edit_brush);
                        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                    }
                    0
                }
                _ => DefWindowProcW(hwnd, message, wparam, lparam),
            }
        }
    }

    unsafe fn paint_settings_window(hwnd: HWND) {
        unsafe {
            let Some(state) = settings_state_from_hwnd(hwnd).as_ref() else {
                return;
            };

            let mut paint: PAINTSTRUCT = zeroed();
            let hdc = BeginPaint(hwnd, &mut paint);
            let mut rect: RECT = zeroed();
            GetClientRect(hwnd, &mut rect);
            FillRect(hdc, &rect, state.background_brush);

            draw_text(
                hdc,
                state.title_font,
                SETTINGS_TEXT,
                RECT {
                    left: 32,
                    top: 22,
                    right: 588,
                    bottom: 62,
                },
                "Manage App",
                DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );
            draw_text(
                hdc,
                state.body_font,
                SETTINGS_MUTED,
                RECT {
                    left: 32,
                    top: 55,
                    right: 588,
                    bottom: 84,
                },
                "Account, polling, and overlay preferences",
                DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );

            draw_card(
                hdc,
                RECT {
                    left: 24,
                    top: 94,
                    right: 596,
                    bottom: 226,
                },
            );
            draw_card(
                hdc,
                RECT {
                    left: 24,
                    top: 244,
                    right: 596,
                    bottom: 326,
                },
            );
            draw_card(
                hdc,
                RECT {
                    left: 24,
                    top: 344,
                    right: 596,
                    bottom: 426,
                },
            );

            draw_input_box(
                hdc,
                RECT {
                    left: 184,
                    top: 134,
                    right: 568,
                    bottom: 176,
                },
            );
            draw_input_box(
                hdc,
                RECT {
                    left: 184,
                    top: 178,
                    right: 568,
                    bottom: 220,
                },
            );
            draw_input_box(
                hdc,
                RECT {
                    left: 184,
                    top: 274,
                    right: 316,
                    bottom: 316,
                },
            );
            draw_input_box(
                hdc,
                RECT {
                    left: 184,
                    top: 374,
                    right: 452,
                    bottom: 416,
                },
            );
            draw_input_box(
                hdc,
                RECT {
                    left: 490,
                    top: 374,
                    right: 568,
                    bottom: 416,
                },
            );

            draw_section_title(hdc, state.section_font, "Credentials", 48, 110);
            draw_field_label(hdc, state.label_font, "Username", 48, 150);
            draw_field_label(hdc, state.label_font, "Password", 48, 188);

            draw_section_title(hdc, state.section_font, "Polling", 48, 260);
            draw_field_label(hdc, state.label_font, "Pull interval", 48, 288);
            draw_text(
                hdc,
                state.body_font,
                SETTINGS_MUTED,
                RECT {
                    left: 316,
                    top: 286,
                    right: 460,
                    bottom: 318,
                },
                "seconds",
                DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );

            draw_section_title(hdc, state.section_font, "Overlay", 48, 360);
            draw_field_label(hdc, state.label_font, "Font family", 48, 388);
            draw_field_label(hdc, state.label_font, "Size", 462, 388);

            EndPaint(hwnd, &paint);
        }
    }

    unsafe fn draw_card(hdc: HDC, rect: RECT) {
        unsafe {
            let brush = CreateSolidBrush(SETTINGS_CARD);
            let pen = CreatePen(PS_SOLID, 1, SETTINGS_BORDER);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);
            RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 18, 18);
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(pen);
            DeleteObject(brush);
        }
    }

    unsafe fn draw_input_box(hdc: HDC, rect: RECT) {
        unsafe {
            let brush = CreateSolidBrush(SETTINGS_INPUT_BG);
            let pen = CreatePen(PS_SOLID, 1, SETTINGS_INPUT_BORDER);
            let old_brush = SelectObject(hdc, brush);
            let old_pen = SelectObject(hdc, pen);
            RoundRect(hdc, rect.left, rect.top, rect.right, rect.bottom, 12, 12);
            SelectObject(hdc, old_pen);
            SelectObject(hdc, old_brush);
            DeleteObject(pen);
            DeleteObject(brush);
        }
    }

    unsafe fn draw_section_title(hdc: HDC, font: HFONT, text: &str, x: i32, y: i32) {
        unsafe {
            draw_text(
                hdc,
                font,
                SETTINGS_ACCENT,
                RECT {
                    left: x,
                    top: y,
                    right: 560,
                    bottom: y + 30,
                },
                text,
                DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }

    unsafe fn draw_field_label(hdc: HDC, font: HFONT, text: &str, x: i32, y: i32) {
        unsafe {
            draw_text(
                hdc,
                font,
                SETTINGS_MUTED,
                RECT {
                    left: x,
                    top: y,
                    right: x + 140,
                    bottom: y + 30,
                },
                text,
                DT_LEFT | DT_TOP | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }

    unsafe fn draw_text(hdc: HDC, font: HFONT, color: u32, mut rect: RECT, text: &str, flags: u32) {
        unsafe {
            let old_font = SelectObject(hdc, font);
            SetBkMode(hdc, TRANSPARENT as i32);
            SetTextColor(hdc, color);
            let text = wide(text);
            DrawTextW(hdc, text.as_ptr(), -1, &mut rect, flags);
            if !old_font.is_null() {
                SelectObject(hdc, old_font);
            }
        }
    }

    unsafe fn settings_brush(hwnd: HWND, hdc: HDC, edit: bool) -> LRESULT {
        unsafe {
            let Some(state) = settings_state_from_hwnd(hwnd).as_ref() else {
                return 0;
            };

            if edit {
                SetTextColor(hdc, SETTINGS_TEXT);
                SetBkColor(hdc, SETTINGS_EDIT_BG);
                state.edit_brush as LRESULT
            } else {
                SetTextColor(hdc, SETTINGS_TEXT);
                SetBkMode(hdc, TRANSPARENT as i32);
                state.background_brush as LRESULT
            }
        }
    }

    unsafe fn draw_owner_button(hwnd: HWND, item: *const DRAWITEMSTRUCT) {
        unsafe {
            if item.is_null() {
                return;
            }

            let item = &*item;
            let Some(state) = settings_state_from_hwnd(hwnd).as_ref() else {
                return;
            };
            let Some(visual) = button_visual(item.CtlID as usize) else {
                return;
            };

            let disabled = item.itemState & ODS_DISABLED != 0;
            let pressed = item.itemState & ODS_SELECTED != 0;
            let hot = item.itemState & ODS_HOTLIGHT != 0;
            let fill = if disabled {
                rgb(235, 238, 242)
            } else if pressed {
                visual.pressed_fill
            } else if hot {
                visual.hot_fill
            } else {
                visual.fill
            };
            let text_color = if disabled {
                rgb(142, 153, 166)
            } else {
                visual.text
            };

            let mut rect = item.rcItem;
            let brush = CreateSolidBrush(fill);
            let pen = CreatePen(PS_SOLID, 1, visual.border);
            let old_brush = SelectObject(item.hDC, brush);
            let old_pen = SelectObject(item.hDC, pen);
            RoundRect(
                item.hDC,
                rect.left,
                rect.top,
                rect.right,
                rect.bottom,
                14,
                14,
            );
            SelectObject(item.hDC, old_pen);
            SelectObject(item.hDC, old_brush);
            DeleteObject(pen);
            DeleteObject(brush);

            if pressed {
                rect.top += 1;
                rect.bottom += 1;
            }

            draw_text(
                item.hDC,
                state.body_font,
                text_color,
                rect,
                visual.label,
                DT_CENTER | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX,
            );
        }
    }

    fn button_visual(id: usize) -> Option<ButtonVisual> {
        match id {
            SETTINGS_SAVE => Some(ButtonVisual {
                label: "Save changes",
                fill: SETTINGS_ACCENT,
                border: SETTINGS_ACCENT,
                text: rgb(255, 255, 255),
                hot_fill: rgb(34, 124, 247),
                pressed_fill: rgb(15, 93, 205),
            }),
            SETTINGS_CLEAR_TOKEN => Some(ButtonVisual {
                label: "Clear token",
                fill: rgb(255, 245, 245),
                border: rgb(255, 200, 200),
                text: rgb(176, 48, 48),
                hot_fill: rgb(255, 236, 236),
                pressed_fill: rgb(255, 225, 225),
            }),
            SETTINGS_RELOAD => Some(ButtonVisual {
                label: "Reload",
                fill: rgb(239, 246, 255),
                border: rgb(194, 220, 255),
                text: rgb(35, 92, 165),
                hot_fill: rgb(228, 239, 255),
                pressed_fill: rgb(214, 230, 252),
            }),
            SETTINGS_CLOSE => Some(ButtonVisual {
                label: "Close",
                fill: rgb(243, 246, 250),
                border: rgb(218, 226, 236),
                text: SETTINGS_TEXT,
                hot_fill: rgb(234, 239, 246),
                pressed_fill: rgb(224, 232, 242),
            }),
            _ => None,
        }
    }

    unsafe fn create_settings_controls(hwnd: HWND, values: &SettingsValues) {
        unsafe {
            let Some(state) = settings_state_from_hwnd(hwnd).as_mut() else {
                return;
            };

            let username = create_edit(
                hwnd,
                SETTINGS_USERNAME,
                Bounds {
                    x: 198,
                    y: 145,
                    width: 356,
                    height: 24,
                },
                &values.username,
                false,
                false,
            );
            let password = create_edit(
                hwnd,
                SETTINGS_PASSWORD,
                Bounds {
                    x: 198,
                    y: 189,
                    width: 356,
                    height: 24,
                },
                &values.password,
                true,
                false,
            );
            let interval = create_edit(
                hwnd,
                SETTINGS_INTERVAL,
                Bounds {
                    x: 198,
                    y: 285,
                    width: 102,
                    height: 24,
                },
                &values.interval_seconds.to_string(),
                false,
                true,
            );
            let font = create_edit(
                hwnd,
                SETTINGS_FONT,
                Bounds {
                    x: 198,
                    y: 385,
                    width: 238,
                    height: 24,
                },
                &values.label_font,
                false,
                false,
            );
            let font_size = create_edit(
                hwnd,
                SETTINGS_FONT_SIZE,
                Bounds {
                    x: 504,
                    y: 385,
                    width: 48,
                    height: 24,
                },
                &values.label_font_size.to_string(),
                false,
                true,
            );

            let clear = create_button(
                hwnd,
                SETTINGS_CLEAR_TOKEN,
                Bounds {
                    x: 32,
                    y: 434,
                    width: 126,
                    height: 34,
                },
                "Clear token",
                false,
            );
            let reload = create_button(
                hwnd,
                SETTINGS_RELOAD,
                Bounds {
                    x: 166,
                    y: 434,
                    width: 84,
                    height: 34,
                },
                "Reload",
                false,
            );
            let close = create_button(
                hwnd,
                SETTINGS_CLOSE,
                Bounds {
                    x: 386,
                    y: 434,
                    width: 82,
                    height: 34,
                },
                "Close",
                false,
            );
            let save = create_button(
                hwnd,
                SETTINGS_SAVE,
                Bounds {
                    x: 476,
                    y: 434,
                    width: 112,
                    height: 34,
                },
                "Save changes",
                true,
            );

            for control in [
                username, password, interval, font, font_size, clear, reload, close, save,
            ] {
                set_control_font(control, state.body_font);
            }

            state.username = username;
            state.password = password;
            state.interval = interval;
            state.font = font;
            state.font_size = font_size;
        }
    }

    unsafe fn save_settings_from_window(hwnd: HWND) {
        unsafe {
            let Some(settings) = settings_state_from_hwnd(hwnd).as_ref() else {
                return;
            };

            let username = get_control_text(settings.username).trim().to_owned();
            let password = get_control_text(settings.password);
            let interval = get_control_text(settings.interval)
                .trim()
                .parse::<u64>()
                .ok();
            let label_font = get_control_text(settings.font).trim().to_owned();
            let label_font_size = get_control_text(settings.font_size)
                .trim()
                .parse::<i32>()
                .ok();

            let Some(interval) = interval.filter(|value| *value > 0) else {
                set_parent_label(
                    settings.parent_hwnd,
                    "Error: Pull interval must be a positive number",
                    LabelKind::Error,
                );
                return;
            };
            let Some(label_font_size) = label_font_size.filter(|value| (8..=72).contains(value))
            else {
                set_parent_label(
                    settings.parent_hwnd,
                    "Error: Label size must be between 8 and 72",
                    LabelKind::Error,
                );
                return;
            };
            let label_font = if label_font.is_empty() {
                DEFAULT_LABEL_FONT.to_owned()
            } else {
                label_font
            };

            let save_result = settings
                .client
                .lock()
                .map_err(|_| "client lock poisoned".to_owned())
                .and_then(|mut client| {
                    let mut env =
                        DotEnvStore::new(&client.env_path).map_err(|error| error.to_string())?;
                    env.set("JAHAN_NAMA_USERNAME", username);
                    env.set("JAHAN_NAMA_PASSWORD", password);
                    env.set("JAHAN_NAMA_INTERVAL_SECONDS", interval.to_string());
                    env.set("JAHAN_NAMA_LABEL_FONT_FAMILY", label_font.clone());
                    env.set("JAHAN_NAMA_LABEL_FONT_SIZE", label_font_size.to_string());
                    env.save().map_err(|error| error.to_string())?;
                    client.client = None;
                    Ok(())
                });

            match save_result {
                Ok(()) => {
                    apply_parent_settings(
                        settings.parent_hwnd,
                        interval,
                        &label_font,
                        label_font_size,
                    );
                    set_parent_label(settings.parent_hwnd, "Settings saved", LabelKind::Normal);
                }
                Err(error) => {
                    set_parent_label(
                        settings.parent_hwnd,
                        &format!("Error: {error}"),
                        LabelKind::Error,
                    );
                }
            }
        }
    }

    unsafe fn clear_token_from_window(hwnd: HWND) {
        unsafe {
            let Some(settings) = settings_state_from_hwnd(hwnd).as_ref() else {
                return;
            };

            let result = settings
                .client
                .lock()
                .map_err(|_| "client lock poisoned".to_owned())
                .and_then(|mut client| client.reset_auth());

            match result {
                Ok(()) => {
                    set_parent_label(settings.parent_hwnd, "Token cleared", LabelKind::Normal)
                }
                Err(error) => {
                    set_parent_label(
                        settings.parent_hwnd,
                        &format!("Error: {error}"),
                        LabelKind::Error,
                    );
                }
            }
        }
    }

    unsafe fn reload_from_settings_window(hwnd: HWND) {
        unsafe {
            let Some(settings) = settings_state_from_hwnd(hwnd).as_ref() else {
                return;
            };

            if let Some(parent_state) = state_from_hwnd(settings.parent_hwnd).as_mut() {
                start_action(
                    settings.parent_hwnd,
                    parent_state,
                    FetchAction::Reload,
                    Some("Reloading..."),
                );
            }
        }
    }

    unsafe fn apply_parent_settings(
        hwnd: HWND,
        interval_seconds: u64,
        font_name: &str,
        font_size: i32,
    ) {
        unsafe {
            if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                state.interval_seconds = interval_seconds;
                state.label_font = font_name.to_owned();
                state.label_font_size = font_size;
                let new_font = create_font(&state.label_font, state.label_font_size);
                if !new_font.is_null() {
                    if !state.font.is_null() {
                        DeleteObject(state.font);
                    }
                    state.font = new_font;
                }
                SetTimer(
                    hwnd,
                    TIMER_ID,
                    interval_seconds
                        .saturating_mul(1000)
                        .clamp(250, u32::MAX as u64) as u32,
                    None,
                );
                resize_for_label(hwnd, &state.label);
                InvalidateRect(hwnd, null(), 1);
            }
        }
    }

    unsafe fn set_parent_label(hwnd: HWND, text: &str, kind: LabelKind) {
        unsafe {
            if let Some(state) = state_from_hwnd(hwnd).as_mut() {
                set_label(hwnd, state, wrap_label_text(text), kind);
            }
        }
    }

    unsafe fn add_tray_icon(hwnd: HWND, icon: HICON) -> bool {
        unsafe {
            let mut data = tray_data(hwnd, icon);
            if Shell_NotifyIconW(NIM_ADD, &data) == 0 {
                return false;
            }
            data.Anonymous.uVersion = NOTIFYICON_VERSION_4;
            Shell_NotifyIconW(NIM_SETVERSION, &data) != 0
        }
    }

    unsafe fn delete_tray_icon(hwnd: HWND) {
        unsafe {
            let data = NOTIFYICONDATAW {
                cbSize: size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: hwnd,
                uID: TRAY_ID,
                ..Default::default()
            };
            Shell_NotifyIconW(NIM_DELETE, &data);
        }
    }

    fn tray_data(hwnd: HWND, icon: HICON) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: WM_TRAY_ICON,
            hIcon: icon,
            ..Default::default()
        };
        copy_wide(&mut data.szTip, "Jahan Nama");
        data
    }

    unsafe fn append_menu_item(
        menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
        id: usize,
        text: &str,
    ) {
        unsafe {
            let text = wide(text);
            AppendMenuW(menu, MF_STRING, id, text.as_ptr());
        }
    }

    unsafe fn create_edit(
        hwnd: HWND,
        id: usize,
        bounds: Bounds,
        text: &str,
        password: bool,
        number: bool,
    ) -> HWND {
        let mut style = WS_CHILD | WS_VISIBLE | WS_TABSTOP | ES_LEFT as u32 | ES_AUTOHSCROLL as u32;
        if password {
            style |= ES_PASSWORD as u32;
        }
        if number {
            style |= ES_NUMBER as u32;
        }

        unsafe {
            CreateWindowExW(
                0,
                wide("EDIT").as_ptr(),
                wide(text).as_ptr(),
                style,
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                hwnd,
                control_id(id),
                GetModuleHandleW(null()),
                null(),
            )
        }
    }

    unsafe fn create_button(
        hwnd: HWND,
        id: usize,
        bounds: Bounds,
        text: &str,
        _primary: bool,
    ) -> HWND {
        unsafe {
            CreateWindowExW(
                0,
                wide("BUTTON").as_ptr(),
                wide(text).as_ptr(),
                WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                hwnd,
                control_id(id),
                GetModuleHandleW(null()),
                null(),
            )
        }
    }

    unsafe fn set_control_font(hwnd: HWND, font: HFONT) {
        unsafe {
            SendMessageW(hwnd, WM_SETFONT, font as WPARAM, 1);
        }
    }

    unsafe fn set_label(hwnd: HWND, state: &mut AppState, text: String, kind: LabelKind) {
        state.label = text;
        state.label_kind = kind;
        unsafe {
            resize_for_label(hwnd, &state.label);
            InvalidateRect(hwnd, null(), 1);
        }
    }

    unsafe fn state_from_hwnd(hwnd: HWND) -> *mut AppState {
        unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut AppState }
    }

    unsafe fn settings_state_from_hwnd(hwnd: HWND) -> *mut SettingsState {
        unsafe { GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut SettingsState }
    }

    unsafe fn get_control_text(hwnd: HWND) -> String {
        unsafe {
            let mut buffer = [0u16; 1024];
            let len = GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
            String::from_utf16_lossy(&buffer[..len.max(0) as usize])
        }
    }

    unsafe fn resize_for_label(hwnd: HWND, label: &str) {
        let lines = label.lines().count().max(1) as i32;
        let height = (lines * 24 + 12).clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT);
        unsafe {
            SetWindowPos(
                hwnd,
                null_mut(),
                0,
                0,
                WINDOW_WIDTH,
                height,
                SWP_NOMOVE | SWP_NOACTIVATE,
            );
        }
    }

    unsafe fn create_font(name: &str, size: i32) -> HFONT {
        unsafe { create_weighted_font(name, size, FW_NORMAL) }
    }

    unsafe fn create_weighted_font(name: &str, size: i32, weight: u32) -> HFONT {
        let family = wide(name);
        let height = -(size.clamp(8, 72) * 96 / 72);
        unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                weight as i32,
                0,
                0,
                0,
                DEFAULT_CHARSET.into(),
                OUT_DEFAULT_PRECIS.into(),
                CLIP_DEFAULT_PRECIS.into(),
                CLEARTYPE_QUALITY.into(),
                (DEFAULT_PITCH | FF_DONTCARE).into(),
                family.as_ptr(),
            )
        }
    }

    unsafe fn delete_object(handle: *mut core::ffi::c_void) {
        unsafe {
            if !handle.is_null() {
                DeleteObject(handle);
            }
        }
    }

    unsafe fn load_app_icon() -> LoadedIcon {
        unsafe {
            for path in icon_candidates() {
                let path = wide_os(path.as_os_str());
                let handle = LoadImageW(
                    null_mut(),
                    path.as_ptr(),
                    IMAGE_ICON,
                    GetSystemMetrics(SM_CXICON),
                    GetSystemMetrics(SM_CYICON),
                    LR_LOADFROMFILE | LR_DEFAULTSIZE,
                ) as HICON;

                if !handle.is_null() {
                    return LoadedIcon {
                        handle,
                        owned: true,
                    };
                }
            }

            LoadedIcon {
                handle: LoadIconW(null_mut(), IDI_APPLICATION),
                owned: false,
            }
        }
    }

    unsafe fn destroy_loaded_icon(icon: LoadedIcon) {
        unsafe {
            if icon.owned && !icon.handle.is_null() {
                DestroyIcon(icon.handle);
            }
        }
    }

    fn icon_candidates() -> Vec<PathBuf> {
        let mut paths = Vec::new();

        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            paths.push(dir.join("icon.ico"));
        }

        if let Ok(dir) = std::env::current_dir() {
            paths.push(dir.join("icon.ico"));
        }

        paths.push(PathBuf::from("icon.ico"));
        paths
    }

    fn load_settings_values(env_path: &PathBuf, fallback_interval: u64) -> SettingsValues {
        let env = DotEnvStore::new(env_path).ok();
        let get = |key: &str| {
            env.as_ref()
                .and_then(|env| env.get(key))
                .unwrap_or_default()
                .to_owned()
        };

        let interval_seconds = get("JAHAN_NAMA_INTERVAL_SECONDS")
            .parse::<u64>()
            .ok()
            .filter(|value| *value > 0)
            .unwrap_or(fallback_interval.max(1));
        let label_font = {
            let value = get("JAHAN_NAMA_LABEL_FONT_FAMILY");
            if value.trim().is_empty() {
                DEFAULT_LABEL_FONT.to_owned()
            } else {
                value
            }
        };
        let label_font_size = get("JAHAN_NAMA_LABEL_FONT_SIZE")
            .parse::<i32>()
            .ok()
            .filter(|value| (8..=72).contains(value))
            .unwrap_or(DEFAULT_LABEL_FONT_SIZE);

        SettingsValues {
            username: get("JAHAN_NAMA_USERNAME"),
            password: get("JAHAN_NAMA_PASSWORD"),
            interval_seconds,
            label_font,
            label_font_size,
        }
    }

    fn wrap_label_text(text: &str) -> String {
        const MAX_CHARS: usize = 78;
        let mut wrapped = Vec::new();

        for source_line in text.lines() {
            let mut current = String::new();

            for word in source_line.split_whitespace() {
                if current.is_empty() {
                    current.push_str(word);
                } else if current.chars().count() + 1 + word.chars().count() <= MAX_CHARS {
                    current.push(' ');
                    current.push_str(word);
                } else {
                    wrapped.push(current);
                    current = word.to_owned();
                }

                while current.chars().count() > MAX_CHARS {
                    let tail = current.chars().skip(MAX_CHARS).collect::<String>();
                    wrapped.push(current.chars().take(MAX_CHARS).collect());
                    current = tail;
                }
            }

            if current.is_empty() {
                wrapped.push(String::new());
            } else {
                wrapped.push(current);
            }
        }

        wrapped.join("\n")
    }

    fn text_color(kind: LabelKind, hover: bool) -> u32 {
        match (kind, hover) {
            (LabelKind::Normal, false) => rgb(0, 128, 0),
            (LabelKind::Normal, true) => rgb(144, 238, 144),
            (LabelKind::Busy, false) => rgb(184, 134, 11),
            (LabelKind::Busy, true) => rgb(255, 215, 0),
            (LabelKind::Error, false) => rgb(220, 80, 80),
            (LabelKind::Error, true) => rgb(255, 140, 140),
        }
    }

    fn copy_wide(target: &mut [u16], value: &str) {
        let encoded = OsStr::new(value).encode_wide();
        let limit = target.len().saturating_sub(1);
        for (slot, code) in target.iter_mut().take(limit).zip(encoded) {
            *slot = code;
        }
    }

    fn wide(value: &str) -> Vec<u16> {
        OsStr::new(value).encode_wide().chain([0]).collect()
    }

    fn wide_os(value: &OsStr) -> Vec<u16> {
        value.encode_wide().chain([0]).collect()
    }

    const fn loword(value: usize) -> usize {
        value & 0xffff
    }

    fn control_id(id: usize) -> HMENU {
        id as isize as HMENU
    }

    #[allow(dead_code)]
    fn string_from_wide(value: &[u16]) -> String {
        let len = value.iter().position(|ch| *ch == 0).unwrap_or(value.len());
        OsString::from_wide(&value[..len])
            .to_string_lossy()
            .into_owned()
    }

    const fn rgb(red: u8, green: u8, blue: u8) -> u32 {
        red as u32 | ((green as u32) << 8) | ((blue as u32) << 16)
    }
}
