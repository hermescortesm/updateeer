//! Installed, History and Settings pages.
use std::sync::mpsc;
use std::thread;

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, ScrollArea, Sense, Stroke, Ui, Vec2};

use std::time::SystemTime;

use crate::model::{self, App};
use crate::scan;
use crate::settings::HOURS;
use crate::theme::*;
use crate::queue::Status;
use crate::ui::{Page, UpdatesApp, cell, search_field, segment};
use crate::widgets::{self, Icon, Kind};

fn header(ui: &mut Ui, title: &str, subtitle: &str) {
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.y = 6.0;
        ui.label(RichText::new(title).size(36.0).family(display()));
        ui.label(RichText::new(subtitle).size(14.0).color(MUTED));
    });
    ui.add_space(22.0);
}

fn card(ui: &mut Ui, add: impl FnOnce(&mut Ui)) {
    Frame::new().fill(PANEL).stroke(Stroke::new(1.0, LINE)).corner_radius(16).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.spacing_mut().item_spacing = Vec2::ZERO;
        add(ui);
    });
}

fn empty(ui: &mut Ui, msg: &str) {
    ui.add_space(64.0);
    ui.vertical_centered(|ui| ui.label(RichText::new(msg).color(FAINT)));
}

/// A 68px list row with hover and divider; returns its rect and the left/right content edges.
fn list_row(ui: &mut Ui) -> (egui::Rect, f32, f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::hover());
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 0, Color32::from_white_alpha(6));
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, Color32::from_white_alpha(10)));
    (rect, rect.left() + 18.0, rect.right() - 18.0)
}

fn name_cell(ui: &mut Ui, rect: egui::Rect, left: f32, app: &App, icon: Option<&egui::TextureHandle>) {
    cell(ui, rect, left, left + 40.0, Layout::left_to_right(Align::Center), |ui| widgets::tile(ui, app, icon));
    cell(ui, rect, left + 54.0, left + 264.0, Layout::top_down(Align::Min), |ui| {
        ui.add_space(15.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add(egui::Label::new(RichText::new(&app.name).size(14.5).family(semibold())).truncate());
        ui.add(egui::Label::new(RichText::new(&app.vendor).size(12.5).color(FAINT)).truncate());
    });
}

impl UpdatesApp {
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
                            ui.label(RichText::new(widgets::short_version(&app.from, 40)).monospace().size(13.0).color(TEXT_2));
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
                ui.label(RichText::new(&app.vendor).size(14.0).color(MUTED));
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
            ScrollArea::vertical().auto_shrink(true).show(ui, |ui| {
                for (title, value) in &rows {
                    value_row(ui, title, value);
                }
            });
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
                            ui.label(RichText::new(model::date(entry.at)).monospace().size(12.5).color(FAINT));
                        });
                        cell(ui, rect, left + 290.0, right - 300.0, Layout::left_to_right(Align::Center), |ui| {
                            ui.spacing_mut().item_spacing.x = 8.0;
                            ui.label(RichText::new(&app.from).monospace().size(13.0).color(FAINT));
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
                            widgets::icon(ui, r, Icon::Arrow, FAINT, 2.2);
                            ui.label(RichText::new(&app.to).monospace().size(13.0).color(ACCENT_TEXT));
                        });
                        cell(ui, rect, right - 150.0, right, Layout::right_to_left(Align::Center), |ui| match &entry.error {
                            None => {
                                ui.label(RichText::new("Actualizada").size(13.0).family(semibold()).color(OK));
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                                widgets::icon(ui, r, Icon::Check, OK, 2.6);
                            }
                            Some(e) => {
                                ui.label(RichText::new("Falló").size(13.0).family(semibold()).color(MUTED)).on_hover_text(e);
                            }
                        });
                    });
                }
            });
        });
    }

    pub(crate) fn settings_page(&mut self, ui: &mut Ui) {
        header(ui, "Ajustes", "Cómo busca Updateeer");
        let before = self.settings;
        card(ui, |ui| {
            setting_row(ui, "Búsqueda automática", "Busca actualizaciones sola mientras la app esté abierta.", |ui| {
                widgets::toggle(ui, &mut self.settings.auto);
            });
            setting_row(ui, "Frecuencia", "Cada cuánto se repite la búsqueda.", |ui| {
                Frame::new()
                    .fill(FIELD)
                    .stroke(Stroke::new(1.0, Color32::from_white_alpha(20)))
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
            setting_row(ui, "Versión", "Updateeer", |ui| {
                ui.label(RichText::new(env!("CARGO_PKG_VERSION")).monospace().size(13.0).color(TEXT_2));
            });
            setting_row(ui, "Homebrew", "Ruta que usa para buscar y actualizar.", |ui| {
                ui.label(RichText::new(scan::brew_path().unwrap_or("no instalado")).monospace().size(13.0).color(TEXT_2));
            });
        });
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
            ui.label(RichText::new(title).size(14.0).family(semibold()).color(TEXT_2));
        });
        cell(ui, rect, left + 220.0, right, Layout::right_to_left(Align::Center), |ui| {
            ui.add(egui::Label::new(RichText::new(value).monospace().size(13.0).color(TEXT)).truncate()).on_hover_text(value);
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
            ui.label(RichText::new(desc).size(12.5).color(FAINT));
        });
        cell(ui, rect, right - 330.0, right, Layout::right_to_left(Align::Center), control);
    });
}
