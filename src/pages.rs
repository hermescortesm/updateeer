//! Installed, History and Settings pages.
use std::sync::mpsc;
use std::thread;

use eframe::egui::{self, Align, Color32, Frame, Layout, RichText, ScrollArea, Sense, Stroke, Ui, Vec2};

use crate::model::App;
use crate::scan;
use crate::settings::HOURS;
use crate::theme::*;
use crate::ui::{UpdatesApp, cell, search_field, segment};
use crate::widgets::{self, Icon};

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

fn name_cell(ui: &mut Ui, rect: egui::Rect, left: f32, app: &App) {
    cell(ui, rect, left, left + 40.0, Layout::left_to_right(Align::Center), |ui| widgets::tile(ui, app));
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
            Ok(apps) => self.installed = apps,
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
            "Leyendo Homebrew…".to_string()
        } else {
            format!("{} apps de Homebrew", self.installed.len())
        };
        header(ui, "Instaladas", &sub);
        ui.horizontal(|ui| ui.with_layout(Layout::right_to_left(Align::Center), |ui| search_field(ui, &mut self.query)));
        ui.add_space(22.0);
        card(ui, |ui| {
            ui.set_height(ui.available_height());
            if rows.is_empty() {
                let msg = match (&self.installed_error, loading) {
                    (Some(e), _) => format!("No se pudo leer: {e}"),
                    (_, true) => "Cargando…".into(),
                    _ if self.installed.is_empty() => "No hay apps instaladas con Homebrew.".into(),
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
                        name_cell(ui, rect, left, app);
                        cell(ui, rect, left + 290.0, right - 100.0, Layout::left_to_right(Align::Center), |ui| {
                            ui.label(RichText::new(&app.from).monospace().size(13.0).color(TEXT_2));
                        });
                        cell(ui, rect, right - 86.0, right, Layout::left_to_right(Align::Center), |ui| widgets::chip(ui, app.source.label()));
                    });
                }
            });
        });
    }

    pub(crate) fn history_page(&mut self, ui: &mut Ui) {
        let n = self.history.len();
        let sub = if n == 0 { "Nada todavía en esta sesión".to_string() } else { format!("{n} en esta sesión") };
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
                        name_cell(ui, rect, left, app);
                        cell(ui, rect, left + 290.0, right - 160.0, Layout::left_to_right(Align::Center), |ui| {
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
