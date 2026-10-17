//! Menu bar icon with the update count, and the actions of its menu.
pub enum Action {
    Open,
    Scan,
    Login,
    Quit,
}

#[cfg(target_os = "macos")]
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
            let status = MenuItem::new("Buscando…", false, None);
            let open = MenuItem::with_id("open", "Abrir Updateeer", true, None);
            let scan = MenuItem::with_id("scan", "Buscar actualizaciones", true, None);
            let login = CheckMenuItem::with_id("login", "Abrir al iniciar sesión", true, login_on, None);
            let quit = MenuItem::with_id("quit", "Salir de Updateeer", true, None);
            let menu = Menu::new();
            let sep = PredefinedMenuItem::separator;
            menu.append_items(&[&status, &sep(), &open, &scan, &sep(), &login, &sep(), &quit]).ok()?;

            let glyph = icons::decode_bytes(include_bytes!("../assets/tray.png"))?;
            let icon = Icon::from_rgba(glyph.rgba, glyph.size[0] as u32, glyph.size[1] as u32).ok()?;
            let icon = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_icon_templated(icon)
                .with_tooltip("Updateeer")
                .build()
                .ok()?;

            let (tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                let action = match e.id.0.as_str() {
                    "open" => Action::Open,
                    "scan" => Action::Scan,
                    "login" => Action::Login,
                    "quit" => Action::Quit,
                    _ => return,
                };
                let _ = tx.send(action);
                ctx.request_repaint();
            }));
            Some(Self { icon, status, login, shown: (usize::MAX, false), rx })
        }

        /// Count next to the icon and the first menu line; cheap to call every frame.
        pub fn update(&mut self, available: usize, scanning: bool) {
            if self.shown == (available, scanning) {
                return;
            }
            self.shown = (available, scanning);
            let text = match (scanning, available) {
                (true, _) => "Buscando actualizaciones…".to_string(),
                (_, 0) => "Todo al día".to_string(),
                (_, 1) => "1 actualización disponible".to_string(),
                (_, n) => format!("{n} actualizaciones disponibles"),
            };
            self.status.set_text(text);
            self.icon.set_title((available > 0).then(|| available.to_string()));
        }

        pub fn set_login(&self, on: bool) {
            self.login.set_checked(on);
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::sync::mpsc;

    use eframe::egui;

    use super::Action;

    /// No menu bar icon on this OS yet.
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

