//! Menu bar (macOS) / notification area (Windows, Linux) icon with the update count, and the actions of its menu.
#[cfg_attr(not(any(target_os = "macos", windows, target_os = "linux")), allow(dead_code))]
pub enum Action {
    Open,
    Scan,
    Login,
    Quit,
}

#[cfg(any(target_os = "macos", windows))]
mod imp {
    use std::sync::mpsc;

    use eframe::egui;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    use super::Action;
    use crate::icons;

    pub struct Tray {
        icon: TrayIcon,
        status: MenuItem,
        login: CheckMenuItem,
        shown: (usize, bool),
        pub rx: mpsc::Receiver<Action>,
    }

    impl Tray {
        pub fn new(ctx: &egui::Context, login_on: bool) -> Option<Self> {
            let status = MenuItem::new(crate::i18n::t("Buscando…"), false, None);
            let open = MenuItem::with_id("open", crate::i18n::t("Abrir Updateeer"), true, None);
            let scan = MenuItem::with_id("scan", crate::i18n::t("Buscar actualizaciones"), true, None);
            let login = CheckMenuItem::with_id("login", crate::i18n::t("Abrir al iniciar sesión"), true, login_on, None);
            let quit = MenuItem::with_id("quit", crate::i18n::t("Salir de Updateeer"), true, None);
            let menu = Menu::new();
            let sep = PredefinedMenuItem::separator;
            menu.append_items(&[&status, &sep(), &open, &scan, &sep(), &login, &sep(), &quit]).ok()?;

            // macOS wants a monochrome template glyph; the Windows notification area shows the colored icon.
            let glyph = icons::decode_bytes(if cfg!(windows) { include_bytes!("../assets/icon.png").as_slice() } else { include_bytes!("../assets/tray.png").as_slice() })?;
            let icon = Icon::from_rgba(glyph.rgba, glyph.size[0] as u32, glyph.size[1] as u32).ok()?;
            let builder = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("Updateeer");
            #[cfg(target_os = "macos")]
            let builder = builder.with_icon_templated(icon);
            // Left click opens the window; the menu is on the right click.
            #[cfg(windows)]
            let builder = builder.with_icon(icon).with_menu_on_left_click(false);
            let icon = builder.build().ok()?;

            let (tx, rx) = mpsc::channel();
            let (menu_tx, menu_ctx) = (tx.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let action = match e.id.0.as_str() {
                    "open" => Action::Open,
                    "scan" => Action::Scan,
                    "login" => Action::Login,
                    "quit" => Action::Quit,
                    _ => return,
                };
                let _ = menu_tx.send(action);
                menu_ctx.request_repaint();
            }));
            #[cfg(windows)]
            {
                let ctx = ctx.clone();
                tray_icon::TrayIconEvent::set_event_handler(Some(move |e: tray_icon::TrayIconEvent| {
                    if let tray_icon::TrayIconEvent::Click { button: tray_icon::MouseButton::Left, button_state: tray_icon::MouseButtonState::Up, .. } = e {
                        let _ = tx.send(Action::Open);
                        ctx.request_repaint();
                    }
                }));
            }
            #[cfg(not(windows))]
            let _ = tx;
            Some(Self { icon, status, login, shown: (usize::MAX, false), rx })
        }

        /// Count next to the icon and the first menu line; cheap to call every frame.
        pub fn update(&mut self, available: usize, scanning: bool) {
            if self.shown == (available, scanning) {
                return;
            }
            self.shown = (available, scanning);
            let text = match (scanning, available) {
                (true, _) => crate::i18n::t("Buscando actualizaciones…").to_string(),
                (_, 0) => crate::i18n::t("Todo al día").to_string(),
                (_, 1) => crate::i18n::t("1 actualización disponible").to_string(),
                (_, n) => crate::i18n::tf1("{0} actualizaciones disponibles", &n.to_string()),
            };
            self.status.set_text(&text);
            // Only macOS can print a number next to the icon; elsewhere it goes in the tooltip.
            #[cfg(target_os = "macos")]
            self.icon.set_title((available > 0).then(|| available.to_string()));
            #[cfg(not(target_os = "macos"))]
            let _ = self.icon.set_tooltip(Some(format!("Updateeer — {text}")));
        }

        pub fn set_login(&self, on: bool) {
            self.login.set_checked(on);
        }
    }
}

/// Linux: a StatusNotifierItem (KDE, most panels, GNOME with the AppIndicator extension), spoken
/// over D-Bus without GTK. When the desktop has no tray the window simply closes, as before.
#[cfg(target_os = "linux")]
mod imp {
    use std::sync::mpsc;

    use eframe::egui;
    use ksni::blocking::{Handle, TrayMethods};
    use ksni::menu::{CheckmarkItem, StandardItem};

    use super::Action;
    use crate::icons;

    struct Model {
        tx: mpsc::Sender<Action>,
        ctx: egui::Context,
        status: String,
        login: bool,
        icon: Vec<ksni::Icon>,
    }

    impl Model {
        fn send(&self, action: Action) {
            let _ = self.tx.send(action);
            self.ctx.request_repaint();
        }
    }

    impl ksni::Tray for Model {
        fn id(&self) -> String {
            "updateeer".into()
        }

        fn title(&self) -> String {
            "Updateeer".into()
        }

        fn icon_pixmap(&self) -> Vec<ksni::Icon> {
            self.icon.clone()
        }

        fn tool_tip(&self) -> ksni::ToolTip {
            ksni::ToolTip { title: format!("Updateeer — {}", self.status), ..Default::default() }
        }

        // Left click opens the window; the menu is on the right click.
        fn activate(&mut self, _x: i32, _y: i32) {
            self.send(Action::Open);
        }

        fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
            let t = crate::i18n::t;
            vec![
                StandardItem { label: self.status.clone(), enabled: false, ..Default::default() }.into(),
                ksni::MenuItem::Separator,
                StandardItem { label: t("Abrir Updateeer").into(), activate: Box::new(|m: &mut Self| m.send(Action::Open)), ..Default::default() }.into(),
                StandardItem { label: t("Buscar actualizaciones").into(), activate: Box::new(|m: &mut Self| m.send(Action::Scan)), ..Default::default() }.into(),
                ksni::MenuItem::Separator,
                CheckmarkItem { label: t("Abrir al iniciar sesión").into(), checked: self.login, activate: Box::new(|m: &mut Self| m.send(Action::Login)), ..Default::default() }.into(),
                ksni::MenuItem::Separator,
                StandardItem { label: t("Salir de Updateeer").into(), activate: Box::new(|m: &mut Self| m.send(Action::Quit)), ..Default::default() }.into(),
            ]
        }
    }

    pub struct Tray {
        handle: Handle<Model>,
        shown: (usize, bool),
        pub rx: mpsc::Receiver<Action>,
    }

    impl Tray {
        pub fn new(ctx: &egui::Context, login_on: bool) -> Option<Self> {
            let glyph = icons::decode_bytes(include_bytes!("../assets/icon.png"))?;
            // ARGB32, network byte order.
            let data = glyph.rgba.as_chunks::<4>().0.iter().flat_map(|p| [p[3], p[0], p[1], p[2]]).collect();
            let icon = vec![ksni::Icon { width: glyph.size[0] as i32, height: glyph.size[1] as i32, data }];
            let (tx, rx) = mpsc::channel();
            let model = Model { tx, ctx: ctx.clone(), status: crate::i18n::t("Buscando…").to_string(), login: login_on, icon };
            // Fails when the desktop has no StatusNotifierWatcher: no tray, the window closes normally.
            let handle = model.spawn().ok()?;
            Some(Self { handle, shown: (usize::MAX, false), rx })
        }

        /// Tooltip and first menu line; cheap to call every frame.
        pub fn update(&mut self, available: usize, scanning: bool) {
            if self.shown == (available, scanning) {
                return;
            }
            self.shown = (available, scanning);
            let text = match (scanning, available) {
                (true, _) => crate::i18n::t("Buscando actualizaciones…").to_string(),
                (_, 0) => crate::i18n::t("Todo al día").to_string(),
                (_, 1) => crate::i18n::t("1 actualización disponible").to_string(),
                (_, n) => crate::i18n::tf1("{0} actualizaciones disponibles", &n.to_string()),
            };
            self.handle.update(|m| m.status = text);
        }

        pub fn set_login(&self, on: bool) {
            self.handle.update(|m| m.login = on);
        }
    }
}

#[cfg(not(any(target_os = "macos", windows, target_os = "linux")))]
mod imp {
    use std::sync::mpsc;

    use eframe::egui;

    use super::Action;

    /// No tray icon on this OS; the window just closes.
    pub struct Tray {
        pub rx: mpsc::Receiver<Action>,
    }

    impl Tray {
        pub fn new(_ctx: &egui::Context, _login_on: bool) -> Option<Self> {
            None
        }
        pub fn update(&mut self, _available: usize, _scanning: bool) {}
        pub fn set_login(&self, _on: bool) {}
    }
}

pub use imp::Tray;
