//! Menu bar (macOS) / notification area (Windows) icon with the update count, and the actions of its menu.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
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
            let status = MenuItem::new("Buscando…", false, None);
            let open = MenuItem::with_id("open", "Abrir Updateeer", true, None);
            let scan = MenuItem::with_id("scan", "Buscar actualizaciones", true, None);
            let login = CheckMenuItem::with_id("login", "Abrir al iniciar sesión", true, login_on, None);
            let quit = MenuItem::with_id("quit", "Salir de Updateeer", true, None);
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
                (true, _) => "Buscando actualizaciones…".to_string(),
                (_, 0) => "Todo al día".to_string(),
                (_, 1) => "1 actualización disponible".to_string(),
                (_, n) => format!("{n} actualizaciones disponibles"),
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

#[cfg(not(any(target_os = "macos", windows)))]
mod imp {
    use std::sync::mpsc;

    use eframe::egui;

    use super::Action;

    /// No tray icon on this OS (it would need GTK); the window just closes.
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
