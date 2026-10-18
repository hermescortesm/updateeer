//! Installed, History and Settings pages.
use std::sync::mpsc;
use std::thread;

use eframe::egui::{self, Align, Frame, Layout, RichText, ScrollArea, Sense, Stroke, Ui, Vec2};

use std::time::SystemTime;

use crate::model::{self, App};
use crate::notify;
use crate::scan;
use crate::settings::{HOURS, ThemeMode};
use crate::theme::*;
use crate::queue::Status;
use crate::catalog::{self, Cat};
use crate::discover;
use crate::ui::{Page, UpdatesApp, cell, combo, search_field, segment};
use crate::widgets::{self, Icon, Kind};

fn header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title).size(36.0).family(display()));
        ui.label(RichText::new(subtitle).size(14.0).color(muted()));
    });
    ui.add_space(22.0);
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new().fill(panel()).stroke(Stroke::new(1.0, line())).corner_radius(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        add(ui);
    });
}

fn empty(ui: &mut Ui, msg: &str) {
    ui.add_space(64.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(msg).color(faint())));
}

/// A 68px list row with hover and divider; returns its rect and the left/right content edges.
fn list_row(ui: &mut Ui) -> (egui::Rect, f32, f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::hover());
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 0, tint(6));
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, tint(10)));
    (rect, rect.left() + 18.0, rect.right() - 18.0)
}

fn name_cell(ui: &mut Ui, rect: egui::Rect, left: f32, app: &App, icon: Option<&egui::TextureHandle>) {
    cell(ui, rect, left, left + 40.0, Layout::left_to_right(Align::Center), |ui| widgets::tile(ui, app, icon));
    cell(ui, rect, left + 54.0, left + 264.0, Layout::top_down(Align::Min), |ui| {
        ui.add_space(15.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add(egui::Label::new(RichText::new(&app.name).size(14.5).family(semibold())).truncate());
        ui.add(egui::Label::new(RichText::new(&app.vendor).size(12.5).color(faint())).truncate());
    });
}

/// Where an install started from "Discover" stands.
pub(crate) enum Inst {
    Running,
    Done,
    Failed(String),
}

type Found = Result<Vec<App>, String>;

/// State of the Discover page.
pub(crate) struct Discover {
    cat: Cat,
    catalog: Vec<App>,
    installs: std::collections::HashMap<String, Inst>,
    tx: mpsc::Sender<(String, Result<(), String>)>,
    rx: mpsc::Receiver<(String, Result<(), String>)>,
    /// A free-text search in flight, and the last one's results (with the text they were for).
    search_rx: Option<mpsc::Receiver<Found>>,
    results: Vec<App>,
    results_for: String,
    search_error: Option<String>,
}

impl Discover {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { cat: Cat::All, catalog: catalog::for_this_os(), installs: Default::default(), tx, rx, search_rx: None, results: Vec::new(), results_for: String::new(), search_error: None }
    }
}

impl UpdatesApp {
    fn poll_discover(&mut self) {
        while let Ok((id, result)) = self.discover.rx.try_recv() {
            match result {
                Ok(()) => {
                    self.discover.installs.insert(id, Inst::Done);
                    self.installed_stale = true;
                }
                Err(e) => {
                    self.discover.installs.insert(id, Inst::Failed(e));
                }
            }
        }
        if let Some(rx) = &self.discover.search_rx
            && let Ok(result) = rx.try_recv()
        {
            self.discover.search_rx = None;
            match result {
                Ok(apps) => self.discover.results = apps,
                Err(e) => self.discover.search_error = Some(e),
            }
        }
    }

    fn start_install(&mut self, id: String) {
        self.discover.installs.insert(id.clone(), Inst::Running);
        let (tx, ctx) = (self.discover.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let result = crate::upgrade::install(&id);
            let _ = tx.send((id, result));
            ctx.request_repaint();
        });
    }

    fn start_search(&mut self, q: String) {
        let (tx, ctx) = (mpsc::channel(), self.ctx.clone());
        let sender = tx.0;
        let query = q.clone();
        thread::spawn(move || {
            let _ = sender.send(discover::search(&query));
            ctx.request_repaint();
        });
        self.discover.search_rx = Some(tx.1);
        self.discover.results.clear();
        self.discover.search_error = None;
        self.discover.results_for = q.to_lowercase();
    }

    pub(crate) fn discover_page(&mut self, ui: &mut Ui) {
        self.poll_discover();
        self.poll_installed();
        // The installed list marks what is already there.
        if self.installed_stale && self.installed_rx.is_none() {
            self.start_installed();
        }
        header(ui, "Descubrir", "Apps populares para instalar con un clic");
        if !discover::available() {
            card(ui, |ui| empty(ui, "Hace falta un gestor de paquetes (Homebrew, winget o Flatpak) para instalar apps."));
            return;
        }
        let q = self.query.trim().to_lowercase();
        let searching = self.discover.search_rx.is_some();
        ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| search_field(ui, &mut self.query)));
        ui.add_space(12.0);
        let mut search = false;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            let options: Vec<(Cat, &str)> = Cat::ALL.iter().map(|c| (*c, c.label())).collect();
            combo(ui, "Categoría", &mut self.discover.cat, &options);
            if discover::safe_query(&q) {
                ui.add_space(8.0);
                let label = if searching { "Buscando…" } else { "Buscar más resultados en línea" };
                search = widgets::button(ui, Some(Icon::Search), label, Kind::Soft, 34.0, !searching).clicked();
            }
        });
        if search {
            self.start_search(q.clone());
        }
        ui.add_space(18.0);

        // Catalog matches first, then whatever the online search found (when it was for this text).
        let cat = self.discover.cat;
        let mut rows: Vec<App> = self.discover.catalog.iter().filter(|a| (cat == Cat::All || catalog::CATALOG.iter().any(|e| e.name == a.name && e.cat == cat)) && (q.is_empty() || a.name.to_lowercase().contains(&q))).cloned().collect();
        if cat == Cat::All && self.discover.results_for == q {
            for a in &self.discover.results {
                if !rows.iter().any(|r| r.id.eq_ignore_ascii_case(&a.id)) {
                    rows.push(a.clone());
                }
            }
        }
        let have: std::collections::HashSet<String> = self.installed.iter().map(|a| a.id.to_lowercase()).collect();
        let mut install: Option<String> = None;
        card(ui, |ui| {
            ui.set_height(ui.available_height());
            if rows.is_empty() {
                let msg = match (&self.discover.search_error, searching) {
                    (Some(e), _) => format!("No se pudo buscar: {e}"),
                    (_, true) => "Buscando…".to_string(),
                    _ => "Nada coincide. Prueba «Buscar más resultados en línea».".to_string(),
                };
                empty(ui, &msg);
                return;
            }
            ScrollArea::vertical().auto_shrink(false).show_rows(ui, 68.0, rows.len(), |ui, range| {
                for app in &rows[range] {
                    ui.push_id(&app.id, |ui| {
                        let (rect, left, right) = list_row(ui);
                        name_cell(ui, rect, left, app, self.icons.get(&app.id));
                        let state = self.discover.installs.get(&app.id);
                        let installed = have.contains(&app.id.to_lowercase()) || matches!(state, Some(Inst::Done));
                        if let Some(Inst::Failed(e)) = state {
                            cell(ui, rect, left + 290.0, right - 150.0, Layout::left_to_right(Align::Center), |ui| {
                                let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(e);
                                ui.add(egui::Label::new(RichText::new(format!("Falló: {first}")).size(12.5).color(egui::Color32::from_rgb(0xFF, 0x7A, 0x6B))).truncate()).on_hover_text(e);
                            });
                        }
                        cell(ui, rect, right - 140.0, right, Layout::right_to_left(Align::Center), |ui| {
                            if installed {
                                ui.label(RichText::new("Instalada").size(13.0).family(semibold()).color(accent_text()));
                            } else if matches!(state, Some(Inst::Running)) {
                                ui.label(RichText::new("Instalando…").size(13.0).color(faint()));
                            } else {
                                let label = if matches!(state, Some(Inst::Failed(_))) { "Reintentar" } else { "Instalar" };
                                if widgets::button(ui, Some(Icon::Download), label, Kind::Soft, 34.0, true).clicked() {
                                    install = Some(app.id.clone());
                                }
                            }
                        });
                    });
                }
            });
        });
        if let Some(id) = install {
            self.start_install(id);
        }
    }

    fn start_installed(&mut self) {
        self.installed_stale = false;
        self.installed_error = None;
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(scan::installed());
            ctx.request_repaint();
        });
        self.installed_rx = Some(rx);
    }

    fn poll_installed(&mut self) {
        let Some(rx) = &self.installed_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.installed_rx = None;
        match result {
            Ok((apps, _)) => {
                self.request_icons(&apps);
                self.installed = apps;
            }
            Err(e) => self.installed_error = Some(e),
        }
    }

    pub(crate) fn installed_page(&mut self, ui: &mut Ui) {
        self.poll_installed();
        if self.installed_stale && self.installed_rx.is_none() {
            self.start_installed();
        }
        let loading = self.installed_rx.is_some();
        let q = self.query.trim().to_lowercase();
        let rows: Vec<usize> =
            (0..self.installed.len()).filter(|&i| q.is_empty() || self.installed[i].name.to_lowercase().contains(&q)).collect();
        let sub = if loading && self.installed.is_empty() {
            "Leyendo tus apps…".to_string()
        } else {
            format!("{} apps instaladas", self.installed.len())
        };
        header(ui, "Instaladas", &sub);
        ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| search_field(ui, &mut self.query)));
        ui.add_space(22.0);
        let mut open: Option<App> = None;
        card(ui, |ui| {
            ui.set_height(ui.available_height());
            if rows.is_empty() {
                let msg = match (&self.installed_error, loading) {
                    (Some(e), _) => format!("No se pudo leer: {e}"),
                    (_, true) => "Cargando…".into(),
                    _ if self.installed.is_empty() => "No se encontraron apps instaladas.".into(),
                    _ => "Ninguna app coincide con tu búsqueda.".into(),
                };
                empty(ui, &msg);
                return;
            }
            // Only the visible rows are laid out, so long lists stay cheap.
            ScrollArea::vertical().auto_shrink(false).show_rows(ui, 68.0, rows.len(), |ui, range| {
                for &i in &rows[range] {
                    let app = &self.installed[i];
                    ui.push_id(&app.id, |ui| {
                        let (rect, left, right) = list_row(ui);
                        if ui.interact(rect, ui.make_persistent_id("open"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            open = Some(app.clone());
                        }
                        name_cell(ui, rect, left, app, self.icons.get(&app.id));
                        cell(ui, rect, left + 290.0, right - 100.0, Layout::left_to_right(Align::Center), |ui| {
                            ui.label(RichText::new(widgets::short_version(&app.from, 40)).monospace().size(13.0).color(text_2()));
                        });
                        cell(ui, rect, right - 86.0, right, Layout::left_to_right(Align::Center), |ui| widgets::chip(ui, app.source.label()));
                    });
                }
            });
        });
        if let Some(app) = open {
            self.open_detail(app, Page::Installed);
        }
    }

    pub(crate) fn open_detail(&mut self, app: App, from: Page) {
        self.detail = Some(app);
        self.back = from;
        self.page = Page::Detail;
    }

    pub(crate) fn detail_page(&mut self, ui: &mut Ui) {
        let Some(app) = self.detail.clone() else {
            self.page = self.back;
            return;
        };
        if widgets::button(ui, None, "Volver", Kind::Ghost, 34.0, true).clicked() {
            self.page = self.back;
            return;
        }
        ui.add_space(22.0);
        let is_update = !app.to.is_empty();
        let status = self.queue.status(&app.id);
        // Same rule as the list: no actions while a scan is about to replace the queue.
        let can_update = is_update && matches!(status, Status::Available | Status::Failed) && !self.scanning();
        ui.horizontal(|ui| {
            widgets::tile_sized(ui, &app, self.icons.get(&app.id), 64.0);
            ui.add_space(8.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new(&app.name).size(36.0).family(display()));
                ui.label(RichText::new(&app.vendor).size(14.0).color(muted()));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let label = if status == Status::Failed { "Reintentar" } else { "Actualizar" };
                if can_update && widgets::button(ui, Some(Icon::Download), label, Kind::Primary, 44.0, true).clicked() {
                    self.queue.enqueue([app.id.as_str()]);
                }
            });
        });
        ui.add_space(22.0);

        let (kind, token) = app.id.split_once(':').unwrap_or(("", &app.id));
        let kind_label = match kind {
            "cask" => "app (cask)",
            "mas" => "app de la tienda",
            "direct" => "descarga directa",
            "formula" => "fórmula",
            _ => "paquete",
        };
        let state = if !is_update {
            "Instalada".to_string()
        } else {
            match status {
                Status::Available => "Actualización disponible".into(),
                Status::Queued => "En cola".into(),
                Status::Updating(p) => format!("Actualizando {}%", p.round()),
                Status::Done => "Actualizada".into(),
                Status::Failed => "Falló".into(),
            }
        };
        let command = match kind {
            "cask" => format!("brew upgrade --cask {token}"),
            "formula" => format!("brew upgrade --formula {token}"),
            "mas" => format!("mas upgrade {token}"),
            _ => String::new(),
        };
        let mut rows: Vec<(&str, String)> = vec![
            ("Origen", format!("{} · {kind_label}", app.source.label())),
            ("Estado", state),
            ("Versión instalada", app.from.clone()),
        ];
        if let Some(t) = app.installed_at {
            let when = format!("{} · {}", model::date(t), model::ago(t, SystemTime::now()));
            rows.push(("Instalada", when));
        }
        if is_update {
            rows.push(("Versión nueva", app.to.clone()));
            rows.push(("Tamaño de descarga", crate::model::format_size(app.size_mb)));
        }
        rows.push(("Paquete", token.to_string()));
        if is_update && !command.is_empty() {
            rows.push(("Comando", command));
        }
        if let Some(e) = self.queue.error(&app.id) {
            rows.push(("Último error", e.to_string()));
        }
        card(ui, |ui| {
            for (title, value) in &rows {
                value_row(ui, title, value);
            }
        });
    }

    pub(crate) fn history_page(&mut self, ui: &mut Ui) {
        let n = self.history.len();
        let sub = if n == 0 { "Todavía no has actualizado nada".to_string() } else { format!("{n} actualizaciones registradas") };
        header(ui, "Historial", &sub);
        card(ui, |ui| {
            ui.set_height(ui.available_height());
            if self.history.is_empty() {
                empty(ui, "Aquí aparecerán las apps que actualices.");
                return;
            }
            ScrollArea::vertical().auto_shrink(false).show_rows(ui, 68.0, n, |ui, range| {
                // Newest first.
                for k in range {
                    let entry = &self.history[n - 1 - k];
                    let app = &entry.app;
                    ui.push_id(k, |ui| {
                        let (rect, left, right) = list_row(ui);
                        name_cell(ui, rect, left, app, self.icons.get(&app.id));
                        cell(ui, rect, right - 290.0, right - 160.0, Layout::right_to_left(Align::Center), |ui| {
                            ui.label(RichText::new(model::date(entry.at)).monospace().size(12.5).color(faint()));
                        });
                        let (vx0, vx1) = (left + 290.0, right - 300.0);
                        // Two versions and an arrow share this cell: 8px gaps, 14px arrow, ~8px per mono char.
                        let max_chars = (((vx1 - vx0 - 30.0) / 2.0) / 8.0).max(6.0) as usize;
                        cell(ui, rect, vx0, vx1, Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(RichText::new(widgets::short_version(&app.from, max_chars)).monospace().size(13.0).color(faint())).on_hover_text(&app.from);
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                            widgets::icon(ui, r, Icon::Arrow, faint(), 2.2);
                            ui.label(RichText::new(widgets::short_version(&app.to, max_chars)).monospace().size(13.0).color(accent_text())).on_hover_text(&app.to);
                        });
                        cell(ui, rect, right - 150.0, right, Layout::right_to_left(Align::Center), |ui| match &entry.error {
                            None => {
                                ui.label(RichText::new("Actualizada").size(13.0).family(semibold()).color(ok()));
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                                widgets::icon(ui, r, Icon::Check, ok(), 2.6);
                            }
                            Some(e) => {
                                ui.label(RichText::new("Falló").size(13.0).family(semibold()).color(muted())).on_hover_text(e);
                            }
                        });
                    });
                }
            });
        });
    }

    /// First-run checklist (macOS): the permission that lets Updateeer modify apps, and the tools it uses.
    pub(crate) fn welcome_page(&mut self, ui: &mut Ui) {
        use crate::permissions::Access;
        header(ui, "Bienvenido a Updateeer", "Antes de actualizar nada, comprueba que todo está listo");
        let (brew, mas) = self.tools;
        let access = self.access;
        let installing = self.mas_install_running();
        let (mut open_perm, mut recheck, mut install_brew, mut install_mas, mut done) = (false, false, false, false, false);
        let status = |ui: &mut Ui, text: &str, ok: bool| {
            ui.label(RichText::new(text).size(13.0).family(semibold()).color(if ok { accent_text() } else { muted() }));
        };
        card(ui, |ui| {
            setting_row(ui, "Modificar apps", "macOS pide el permiso «Gestión de apps» para que Updateeer pueda actualizar las apps de Aplicaciones. Actívalo en Ajustes del Sistema → Privacidad y seguridad.", |ui| {
                if access == Access::Granted {
                    status(ui, "Listo", true);
                } else {
                    open_perm = widgets::button(ui, None, "Abrir ajustes", Kind::Primary, 34.0, true).clicked();
                    recheck = widgets::button(ui, None, "Comprobar", Kind::Soft, 34.0, true).clicked();
                    status(ui, if access == Access::Denied { "Falta" } else { "Sin comprobar" }, false);
                }
            });
            setting_row(ui, "Homebrew", "Lo usa para buscar y actualizar apps y herramientas. Se instala desde Terminal y pide tu contraseña.", |ui| {
                if brew {
                    status(ui, "Instalado", true);
                } else {
                    install_brew = widgets::button(ui, None, "Instalar Homebrew", Kind::Primary, 34.0, true).clicked();
                    status(ui, "Falta", false);
                }
            });
            setting_row(ui, "mas (App Store)", "Herramienta pequeña, se instala con Homebrew. Sin ella no se ven las actualizaciones de la App Store.", |ui| {
                if mas {
                    status(ui, "Instalado", true);
                } else {
                    let label = if installing { "Instalando…" } else { "Instalar mas" };
                    install_mas = widgets::button(ui, None, label, Kind::Primary, 34.0, brew && !installing).clicked();
                    status(ui, if brew { "Falta" } else { "Necesita Homebrew" }, false);
                }
            });
        });
        ui.add_space(14.0);
        if let Some(e) = &self.setup_error {
            ui.label(RichText::new(e).size(13.0).color(muted()));
            ui.add_space(10.0);
        }
        ui.horizontal(|ui| {
            done = widgets::button(ui, None, "Continuar", Kind::Primary, 40.0, true).clicked();
            ui.label(RichText::new("Puedes volver aquí desde Ajustes → Permisos.").size(12.5).color(faint()));
        });
        if open_perm {
            crate::permissions::open_settings();
        }
        if recheck {
            self.access = crate::permissions::app_management();
        }
        if install_brew {
            self.install_tool(false);
        }
        if install_mas {
            self.install_tool(true);
        }
        if done {
            self.settings.onboarded = true;
            self.settings.save();
            self.page = Page::Updates;
        }
    }

    pub(crate) fn settings_page(&mut self, ui: &mut Ui) {
        header(ui, "Ajustes", "Cómo busca Updateeer");
        let before = self.settings;
        let mut want_login = None;
        let mut want_welcome = false;
        let mut install_now = false;
        card(ui, |ui| {
            setting_row(ui, "Búsqueda automática", "Busca actualizaciones sola mientras la app esté abierta.", |ui| {
                widgets::toggle(ui, &mut self.settings.auto);
            });
            if notify::supported() {
                setting_row(ui, "Actualizar solas las apps cerradas", "Las que estén abiertas te avisan con una notificación: actualizar ahora o más tarde.", |ui| {
                    widgets::toggle(ui, &mut self.settings.auto_update);
                });
            }
            setting_row(ui, "Avisar de versiones nuevas de Updateeer", "Consulta GitHub al buscar actualizaciones. Es la única conexión que hace la app por su cuenta.", |ui| {
                widgets::toggle(ui, &mut self.settings.check_updates);
            });
            setting_row(ui, "Frecuencia", "Cada cuánto se repite la búsqueda.", |ui| {
                Frame::new()
                    .fill(field())
                    .stroke(Stroke::new(1.0, tint(20)))
                    .corner_radius(12)
                    .inner_margin(4)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.horizontal(|ui| {
                            for h in HOURS {
                                if segment(ui, &format!("{h} h"), self.settings.hours == h).clicked() {
                                    self.settings.hours = h;
                                }
                            }
                        });
                    });
            });
            if crate::autostart::supported() {
                let desc = self.login_error.clone().unwrap_or_else(|| "Se abre en la barra de menú al encender el equipo.".into());
                let mut on = self.login;
                setting_row(ui, "Abrir al iniciar sesión", &desc, |ui| {
                    if widgets::toggle(ui, &mut on).changed() {
                        want_login = Some(on);
                    }
                });
            }
            setting_row(ui, "Apariencia", "Claro, oscuro o el del sistema.", |ui| {
                Frame::new().fill(field()).stroke(Stroke::new(1.0, tint(20))).corner_radius(12).inner_margin(4).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    ui.horizontal(|ui| {
                        for (mode, label) in [(ThemeMode::System, "Sistema"), (ThemeMode::Light, "Claro"), (ThemeMode::Dark, "Oscuro")] {
                            if segment(ui, label, self.settings.theme == mode).clicked() {
                                self.settings.theme = mode;
                            }
                        }
                    });
                });
            });
            if cfg!(target_os = "macos") {
                setting_row(ui, "Permisos", "Gestión de apps, Homebrew y mas: lo que necesita para actualizar.", |ui| {
                    want_welcome = widgets::button(ui, None, "Revisar", Kind::Soft, 34.0, true).clicked();
                });
            }
            let can_install = crate::selfupdate::can_self_install();
            let installing = self.self_update_rx.is_some();
            let version_note = match (&self.new_release, &self.self_update_error) {
                (_, Some(e)) => format!("No se pudo actualizar: {e}"),
                (Some(_), _) if installing => "Descargando e instalando… se te pedirá la contraseña".to_string(),
                (Some(r), _) => format!("Hay una versión nueva: {}", r.version),
                _ => "Updateeer".to_string(),
            };
            setting_row(ui, "Versión", &version_note, |ui| {
                if let Some(r) = &self.new_release {
                    let label = if can_install { "Instalar y reiniciar" } else { "Descargar" };
                    if widgets::button(ui, Some(Icon::Download), label, Kind::Primary, 34.0, !installing).clicked() {
                        if can_install {
                            install_now = true;
                        } else {
                            crate::selfupdate::open(&r.url);
                        }
                    }
                }
                ui.label(RichText::new(env!("CARGO_PKG_VERSION")).monospace().size(13.0).color(text_2()));
            });
            if cfg!(target_os = "macos") {
                setting_row(ui, "Homebrew", "Ruta que usa para buscar y actualizar.", |ui| {
                    ui.label(RichText::new(scan::brew_path().unwrap_or("no instalado")).monospace().size(13.0).color(text_2()));
                });
            }
        });
        ui.add_space(16.0);
        card(ui, |ui| {
            Frame::new().inner_margin(18).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 14.0;
                    widgets::logo(ui, 52.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 3.0;
                        ui.label(RichText::new("Updateeer").size(18.0).family(display()));
                        ui.label(RichText::new("Todas tus apps al día, desde una sola cola. Gratis y de código abierto.").size(12.5).color(faint()));
                    });
                });
            });
            value_row(ui, "Autor", "Hermes Cortes · admin@hermescortes.com");
            value_row(ui, "Licencia", "MIT — gratis para usar, copiar y modificar");
            value_row(ui, "Código fuente", "github.com/hermescortesm/updateeer");
        });
        if install_now {
            self.start_self_update();
        }
        if want_welcome {
            self.access = crate::permissions::app_management();
            self.page = Page::Welcome;
        }
        if let Some(on) = want_login {
            self.set_login(on);
        }
        if self.settings != before {
            self.settings.save();
        }
    }
}

/// A label on the left, a (possibly long) monospace value on the right; the full value shows on hover.
fn value_row(ui: &mut Ui, title: &str, value: &str) {
    ui.push_id(title, |ui| {
        let (rect, left, right) = list_row(ui);
        let ltr = Layout::left_to_right(Align::Center);
        cell(ui, rect, left, left + 200.0, ltr, |ui| {
            ui.label(RichText::new(title).size(14.0).family(semibold()).color(text_2()));
        });
        cell(ui, rect, left + 220.0, right, Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(value).monospace().size(13.0).color(text())).truncate()).on_hover_text(value);
        });
    });
}

/// Title and description on the left, control on the right.
fn setting_row(ui: &mut Ui, title: &str, desc: &str, control: impl FnOnce(&mut Ui)) {
    ui.push_id(title, |ui| {
        let (rect, left, right) = list_row(ui);
        cell(ui, rect, left, right - 340.0, Layout::top_down(Align::Min), |ui| {
            ui.add_space(15.0);
            ui.spacing_mut().item_spacing.y = 3.0;
            ui.label(RichText::new(title).size(14.5).family(semibold()));
            ui.label(RichText::new(desc).size(12.5).color(faint()));
        });
        cell(ui, rect, right - 330.0, right, Layout::right_to_left(Align::Center), control);
    });
}
