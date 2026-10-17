use std::collections::HashSet;

use eframe::egui::{
    self, Align, Align2, Button, CornerRadius, FontId, Frame, Layout, Margin, RichText, ScrollArea,
    Sense, Stroke, StrokeKind, TextEdit, Vec2,
};

use crate::model::{self, App, Source};
use crate::theme::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Only(Source),
}

pub struct UpdatesApp {
    apps: Vec<App>,
    selected: HashSet<String>,
    query: String,
    filter: Filter,
}

impl UpdatesApp {
    pub fn new() -> Self {
        Self {
            apps: model::mock_apps(),
            selected: HashSet::new(),
            query: String::new(),
            filter: Filter::All,
        }
    }

    fn visible(&self) -> Vec<&App> {
        let q = self.query.trim().to_lowercase();
        self.apps
            .iter()
            .filter(|a| match self.filter {
                Filter::All => true,
                Filter::Only(s) => a.source == s,
            })
            .filter(|a| q.is_empty() || a.name.to_lowercase().contains(&q))
            .collect()
    }
}

impl eframe::App for UpdatesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::Panel::left("nav")
            .exact_size(236.0)
            .resizable(false)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(14, 16)))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin { left: 40, right: 40, top: 40, bottom: 28 }))
            .show(ui, |ui| self.main(ui));
    }
}

impl UpdatesApp {
    fn sidebar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            logo(ui, 26.0);
            ui.label(RichText::new("Updateeer").size(20.0).strong());
        });
        ui.add_space(22.0);
        let count = self.apps.len();
        nav_item(ui, "Actualizaciones", true, Some(count));
        nav_item(ui, "Instaladas", false, None);
        nav_item(ui, "Historial", false, None);
        nav_item(ui, "Ajustes", false, None);
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            Frame::new()
                .fill(BTN)
                .stroke(Stroke::new(1.0, LINE))
                .corner_radius(12)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.label(RichText::new("Búsqueda automática").size(12.0).strong().color(TEXT_2));
                    ui.label(RichText::new("Cada 6 horas, en segundo plano").size(12.0).color(FAINT));
                });
        });
    }

    fn main(&mut self, ui: &mut egui::Ui) {
        let available = self.apps.len();
        let total_mb: u32 = self.apps.iter().map(|a| a.size_mb).sum();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Actualizaciones").size(36.0).strong());
                ui.label(
                    RichText::new(format!("{available} disponibles · {}", model::format_size(total_mb)))
                        .size(14.0)
                        .color(MUTED),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                // ponytail: buttons wired in feature/update-queue.
                ui.add(primary_button("Actualizar todo"));
                ui.add(Button::new(RichText::new("Buscar actualizaciones").strong()).min_size(Vec2::new(0.0, 44.0)));
            });
        });
        ui.add_space(22.0);

        ui.horizontal(|ui| {
            ui.add(
                TextEdit::singleline(&mut self.query)
                    .hint_text("Buscar app…")
                    .desired_width(ui.available_width() - 420.0)
                    .margin(Margin::symmetric(14, 11)),
            );
            Frame::new().fill(FIELD).corner_radius(12).inner_margin(4).show(ui, |ui| {
                for (label, f) in [
                    ("Todas", Filter::All),
                    ("App Store", Filter::Only(Source::AppStore)),
                    ("Homebrew", Filter::Only(Source::Homebrew)),
                    ("Directas", Filter::Only(Source::Direct)),
                ] {
                    let on = self.filter == f;
                    let b = Button::new(RichText::new(label).size(13.0).strong().color(if on { TEXT } else { FAINT }))
                        .fill(if on { Color32::from_rgb(0x2A, 0x2D, 0x33) } else { Color32::TRANSPARENT })
                        .min_size(Vec2::new(0.0, 34.0));
                    if ui.add(b).clicked() {
                        self.filter = f;
                    }
                }
            });
        });
        ui.add_space(22.0);

        let visible_ids: Vec<String> = self.visible().iter().map(|a| a.id.clone()).collect();
        Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1.0, LINE))
            .corner_radius(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                let all_on = !visible_ids.is_empty() && visible_ids.iter().all(|id| self.selected.contains(id));
                ui.allocate_ui(Vec2::new(ui.available_width(), 52.0), |ui| {
                    ui.horizontal_centered(|ui| {
                        ui.add_space(18.0);
                        let mut on = all_on;
                        if ui.checkbox(&mut on, "").changed() {
                            for id in &visible_ids {
                                if on { self.selected.insert(id.clone()); } else { self.selected.remove(id); }
                            }
                        }
                        let n = self.selected.len();
                        let label = match n {
                            0 => "Seleccionar todas".to_string(),
                            1 => "1 seleccionada".to_string(),
                            n => format!("{n} seleccionadas"),
                        };
                        ui.label(RichText::new(label).size(13.0).strong().color(TEXT_2));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.add_space(18.0);
                            ui.add_enabled(n > 0, primary_button("Actualizar seleccionadas").min_size(Vec2::new(0.0, 34.0)));
                        });
                    });
                });
                ui.separator();
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    let rows: Vec<App> = self.visible().into_iter().cloned().collect();
                    if rows.is_empty() {
                        ui.add_space(64.0);
                        ui.vertical_centered(|ui| {
                            ui.label(RichText::new("Ninguna app coincide con tu búsqueda.").color(FAINT));
                        });
                    }
                    for app in &rows {
                        self.row(ui, app);
                    }
                });
            });
    }

    fn row(&mut self, ui: &mut egui::Ui, app: &App) {
        ui.allocate_ui(Vec2::new(ui.available_width(), 68.0), |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(18.0);
                let mut on = self.selected.contains(&app.id);
                if ui.checkbox(&mut on, "").changed() {
                    if on { self.selected.insert(app.id.clone()); } else { self.selected.remove(&app.id); }
                }
                tile(ui, app);
                ui.vertical(|ui| {
                    ui.set_width(210.0);
                    ui.label(RichText::new(&app.name).size(14.5).strong());
                    ui.label(RichText::new(&app.vendor).size(12.5).color(FAINT));
                });
                ui.label(RichText::new(&app.from).monospace().size(13.0).color(FAINT));
                ui.label(RichText::new("→").size(13.0).color(FAINT));
                ui.label(RichText::new(&app.to).monospace().size(13.0).color(ACCENT_TEXT));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(18.0);
                    ui.add(Button::new(RichText::new("Actualizar").size(13.0).strong()).min_size(Vec2::new(0.0, 34.0)));
                    ui.add_space(8.0);
                    ui.label(RichText::new(model::format_size(app.size_mb)).monospace().size(12.5).color(FAINT));
                    ui.add_space(8.0);
                    chip(ui, app.source.label());
                });
            });
        });
        ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, Color32::from_white_alpha(10)));
    }
}

use eframe::egui::Color32;

fn primary_button(text: &str) -> Button<'static> {
    Button::new(RichText::new(text).size(14.0).strong().color(ON_ACCENT))
        .fill(ACCENT)
        .min_size(Vec2::new(0.0, 44.0))
}

fn nav_item(ui: &mut egui::Ui, label: &str, active: bool, badge: Option<usize>) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    if active {
        ui.painter().rect_filled(rect, CornerRadius::same(10), Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 31));
    }
    let color = if active { ACCENT_TEXT } else { MUTED };
    ui.painter().text(rect.left_center() + Vec2::new(12.0, 0.0), Align2::LEFT_CENTER, label, FontId::proportional(14.0), color);
    if let Some(n) = badge {
        let b = egui::Rect::from_center_size(rect.right_center() - Vec2::new(22.0, 0.0), Vec2::new(24.0, 22.0));
        ui.painter().rect_filled(b, CornerRadius::same(11), ACCENT);
        ui.painter().text(b.center(), Align2::CENTER_CENTER, n.to_string(), FontId::proportional(12.0), ON_ACCENT);
    }
}

fn logo(ui: &mut egui::Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((size * 0.25) as u8), ACCENT);
    let s = size / 32.0;
    for (i, alpha) in [255u8, 158, 77].into_iter().enumerate() {
        let y = 13.0 + 6.0 * i as f32;
        let pts = [rect.min + Vec2::new(10.0, y) * s, rect.min + Vec2::new(16.0, y - 5.5) * s, rect.min + Vec2::new(22.0, y) * s];
        let c = Color32::from_rgba_unmultiplied(0x1A, 0x0F, 0x05, alpha);
        p.line_segment([pts[0], pts[1]], Stroke::new(3.0 * s, c));
        p.line_segment([pts[1], pts[2]], Stroke::new(3.0 * s, c));
    }
}

fn tile(ui: &mut egui::Ui, app: &App) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(11), tile_color(app.hue));
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        app.name.chars().next().unwrap_or('?'),
        FontId::proportional(17.0),
        Color32::WHITE,
    );
}

fn chip(ui: &mut egui::Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(86.0, 26.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(7), Color32::from_white_alpha(13));
    ui.painter().rect_stroke(rect, CornerRadius::same(7), Stroke::NONE, StrokeKind::Inside);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::proportional(12.0), TEXT_2);
}
