use std::collections::HashSet;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};
use std::thread;

use eframe::egui::{
    self, Align, Checkbox, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder, Vec2,
};

use crate::model::{self, App, Source};
use crate::queue::{Queue, Status};
use crate::settings::Settings;
use crate::{scan, upgrade};
use crate::theme::*;
use crate::widgets::{self, Icon, Kind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Filter {
    All,
    Only(Source),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Updates,
    Installed,
    History,
    Settings,
}

/// One finished upgrade of this session.
pub struct Entry {
    pub app: App,
    pub error: Option<String>,
}

type Scan = Receiver<Result<Vec<App>, String>>;

pub struct UpdatesApp {
    apps: Vec<App>,
    selected: HashSet<String>,
    pub(crate) query: String,
    filter: Filter,
    queue: Queue,
    pub(crate) ctx: egui::Context,
    scan_rx: Option<Scan>,
    scan_error: Option<String>,
    done_tx: Sender<(String, Result<(), String>)>,
    done_rx: Receiver<(String, Result<(), String>)>,
    last_scan: Instant,
    pub(crate) page: Page,
    pub(crate) settings: Settings,
    pub(crate) installed: Vec<App>,
    pub(crate) installed_rx: Option<Scan>,
    pub(crate) installed_stale: bool,
    pub(crate) installed_error: Option<String>,
    pub(crate) history: Vec<Entry>,
}

const CONCURRENCY: usize = 2;

impl UpdatesApp {
    pub fn new(ctx: egui::Context) -> Self {
        let (done_tx, done_rx) = mpsc::channel();
        let mut app = Self {
            apps: Vec::new(),
            selected: HashSet::new(),
            query: String::new(),
            filter: Filter::All,
            queue: Queue::new([], CONCURRENCY),
            ctx,
            scan_rx: None,
            scan_error: None,
            done_tx,
            done_rx,
            last_scan: Instant::now(),
            page: Page::Updates,
            settings: Settings::load(),
            installed: Vec::new(),
            installed_rx: None,
            installed_stale: true,
            installed_error: None,
            history: Vec::new(),
        };
        app.start_scan();
        app
    }

    fn scanning(&self) -> bool {
        self.scan_rx.is_some()
    }

    fn start_scan(&mut self) {
        if self.scanning() || self.queue.is_busy() {
            return;
        }
        self.last_scan = Instant::now();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(scan::scan());
            ctx.request_repaint();
        });
        self.scan_rx = Some(rx);
        self.scan_error = None;
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.scan_rx = None;
        match result {
            Ok(apps) => {
                self.queue = Queue::new(apps.iter().map(|a| a.id.clone()), CONCURRENCY);
                self.apps = apps;
                self.selected.clear();
            }
            Err(e) => self.scan_error = Some(e),
        }
    }

    /// Indices into `apps` that pass the search and source filter.
    fn visible(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        (0..self.apps.len())
            .filter(|&i| {
                let a = &self.apps[i];
                (match self.filter {
                    Filter::All => true,
                    Filter::Only(s) => a.source == s,
                }) && (q.is_empty() || a.name.to_lowercase().contains(&q))
            })
            .collect()
    }
}

impl eframe::App for UpdatesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_scan();
        while let Ok((id, result)) = self.done_rx.try_recv() {
            let error = result.as_ref().err().cloned();
            self.queue.finish(&id, result);
            // A failed first attempt is retried silently: only final outcomes make history.
            let app = self.apps.iter().find(|a| a.id == id);
            match (self.queue.status(&id), app) {
                (Status::Done, Some(a)) => {
                    self.history.push(Entry { app: a.clone(), error: None });
                    self.installed_stale = true;
                }
                (Status::Failed, Some(a)) => self.history.push(Entry { app: a.clone(), error }),
                _ => {}
            }
        }
        if self.settings.auto {
            if self.last_scan.elapsed() >= Duration::from_secs(self.settings.hours as u64 * 3600) {
                self.start_scan();
            }
            ui.request_repaint_after(Duration::from_secs(60));
        }
        if self.queue.is_busy() {
            self.queue.creep(ui.input(|i| i.stable_dt));
            for id in self.queue.start_ready() {
                let (tx, ctx) = (self.done_tx.clone(), self.ctx.clone());
                thread::spawn(move || {
                    let result = upgrade::run(&id);
                    let _ = tx.send((id, result));
                    ctx.request_repaint();
                });
            }
            ui.request_repaint_after(Duration::from_millis(100));
        }
        egui::Panel::left("nav")
            .exact_size(236.0)
            .resizable(false)
            .frame(Frame::new().fill(PANEL).inner_margin(Margin::symmetric(14, 16)))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin { left: 40, right: 40, top: 40, bottom: 28 }))
            .show(ui, |ui| match self.page {
                Page::Updates => self.main(ui),
                Page::Installed => self.installed_page(ui),
                Page::History => self.history_page(ui),
                Page::Settings => self.settings_page(ui),
            });
    }
}

impl UpdatesApp {
    fn sidebar(&mut self, ui: &mut Ui) {
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            widgets::logo(ui, 26.0);
            ui.label(RichText::new("Updateeer").size(20.0).family(display()));
        });
        ui.add_space(22.0);
        ui.spacing_mut().item_spacing.y = 2.0;
        let badge = Some(self.queue.available_count());
        for (page, icon, label) in [
            (Page::Updates, Icon::Download, "Actualizaciones"),
            (Page::Installed, Icon::Grid, "Instaladas"),
            (Page::History, Icon::Clock, "Historial"),
            (Page::Settings, Icon::Sliders, "Ajustes"),
        ] {
            let badge = if page == Page::Updates { badge } else { None };
            if widgets::nav_item(ui, icon, label, self.page == page, badge).clicked() {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            Frame::new()
                .fill(Color32::from_rgb(0x17, 0x19, 0x1C))
                .stroke(Stroke::new(1.0, Color32::from_white_alpha(15)))
                .corner_radius(12)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui.label(RichText::new("Búsqueda automática").size(12.0).family(semibold()).color(TEXT_2));
                    let text = if self.settings.auto { format!("Cada {} h, con la app abierta", self.settings.hours) } else { "Desactivada".into() };
                    ui.label(RichText::new(text).size(12.0).color(FAINT));
                });
        });
    }

    fn main(&mut self, ui: &mut Ui) {
        let available = self.queue.available_count();
        let total_mb: u32 = self
            .apps
            .iter()
            .filter(|a| self.queue.status(&a.id) == Status::Available)
            .filter_map(|a| a.size_mb)
            .sum();
        let summary = if self.scanning() {
            "Buscando actualizaciones…".to_string()
        } else if let Some(e) = &self.scan_error {
            format!("No se pudo buscar: {e}")
        } else if available > 0 {
            let size = if total_mb > 0 { format!(" · {}", model::format_size(Some(total_mb))) } else { String::new() };
            format!("{available} disponibles{size}")
        } else if self.queue.is_busy() {
            "Actualizando…".to_string()
        } else if self.queue.failed_count() > 0 {
            format!("{} fallidas", self.queue.failed_count())
        } else {
            "Todo al día".to_string()
        };
        // No actions while a scan runs: its result replaces the queue.
        let can_act = !self.scanning();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new("Actualizaciones").size(36.0).family(display()));
                ui.label(RichText::new(summary).size(14.0).color(MUTED));
            });
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if widgets::button(ui, Some(Icon::Download), "Actualizar todo", Kind::Primary, 44.0, available > 0 && can_act).clicked() {
                    let ids: Vec<String> = self.queue.available().map(String::from).collect();
                    self.queue.enqueue(ids.iter().map(String::as_str));
                    self.selected.clear();
                }
                let idle = !self.scanning() && !self.queue.is_busy();
                if widgets::button(ui, Some(Icon::Refresh), "Buscar actualizaciones", Kind::Ghost, 44.0, idle).clicked() {
                    self.start_scan();
                }
            });
        });
        ui.add_space(22.0);

        if self.queue.is_busy() {
            self.batch_banner(ui);
            ui.add_space(22.0);
        }

        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                Frame::new()
                    .fill(FIELD)
                    .stroke(Stroke::new(1.0, Color32::from_white_alpha(20)))
                    .corner_radius(12)
                    .inner_margin(4)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.horizontal(|ui| {
                            for (label, f) in [
                                ("Todas", Filter::All),
                                ("App Store", Filter::Only(Source::AppStore)),
                                ("Homebrew", Filter::Only(Source::Homebrew)),
                                ("Directas", Filter::Only(Source::Direct)),
                            ] {
                                if segment(ui, label, self.filter == f).clicked() {
                                    self.filter = f;
                                }
                            }
                        });
                    });
                search_field(ui, &mut self.query);
            });
        });
        ui.add_space(22.0);

        let rows = self.visible();
        let visible_ids: Vec<String> = rows
            .iter()
            .map(|&i| &self.apps[i])
            .filter(|a| self.queue.status(&a.id) == Status::Available)
            .map(|a| a.id.clone())
            .collect();
        let mut to_update: Vec<String> = Vec::new();
        Frame::new()
            .fill(PANEL)
            .stroke(Stroke::new(1.0, LINE))
            .corner_radius(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let selected_visible: Vec<&String> = visible_ids.iter().filter(|id| self.selected.contains(*id)).collect();
                let n = selected_visible.len();
                let all_on = !visible_ids.is_empty() && n == visible_ids.len();
                let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
                ui.painter().hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, LINE));
                let mut inner = ui.new_child(UiBuilder::new().max_rect(head.shrink2(Vec2::new(18.0, 0.0))).layout(Layout::left_to_right(Align::Center)));
                let mut on = all_on;
                if inner.add_enabled(!visible_ids.is_empty(), Checkbox::new(&mut on, "")).changed() {
                    for id in &visible_ids {
                        if on { self.selected.insert(id.clone()); } else { self.selected.remove(id); }
                    }
                }
                let label = match n {
                    0 => "Seleccionar todas".to_string(),
                    1 => "1 seleccionada".to_string(),
                    n => format!("{n} seleccionadas"),
                };
                inner.label(RichText::new(label).size(13.0).family(semibold()).color(TEXT_2));
                inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, None, "Actualizar seleccionadas", Kind::Primary, 34.0, n > 0 && can_act).clicked() {
                        to_update.extend(visible_ids.iter().filter(|id| self.selected.contains(*id)).cloned());
                    }
                });
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if rows.is_empty() {
                        ui.add_space(64.0);
                        ui.vertical_centered(|ui| {
                            let (msg, color) = match (&self.scan_error, self.scanning(), self.apps.is_empty()) {
                                (Some(e), false, _) => (format!("No se pudo buscar: {e}"), MUTED),
                                (_, true, _) => ("Buscando…".into(), FAINT),
                                (_, _, true) => ("No hay actualizaciones.".into(), FAINT),
                                _ => ("Ninguna app coincide con tu búsqueda.".into(), FAINT),
                            };
                            ui.label(RichText::new(msg).color(color));
                        });
                    }
                    for &i in &rows {
                        let app = &self.apps[i];
                        let status = self.queue.status(&app.id);
                        let mut sel = self.selected.contains(&app.id);
                        let was = sel;
                        let err = self.queue.error(&app.id);
                        if row(ui, app, status, err, &mut sel, can_act) {
                            to_update.push(app.id.clone());
                        }
                        if sel != was {
                            if sel { self.selected.insert(app.id.clone()); } else { self.selected.remove(&app.id); }
                        }
                    }
                });
            });
        if !to_update.is_empty() {
            self.queue.enqueue(to_update.iter().map(String::as_str));
            for id in &to_update {
                self.selected.remove(id);
            }
        }
    }

    fn batch_banner(&mut self, ui: &mut Ui) {
        let (done, total, pct) = self.queue.batch_progress();
        Frame::new()
            .fill(Color32::from_rgb(0x1A, 0x15, 0x10))
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 77)))
            .corner_radius(14)
            .inner_margin(Margin::symmetric(18, 16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 5.0, ACCENT);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 110.0);
                        ui.spacing_mut().item_spacing.y = 8.0;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(format!("Actualizadas {done} de {total}")).size(14.0).family(semibold()));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(format!("{}%", pct.round())).monospace().color(ACCENT_TEXT));
                            });
                        });
                        widgets::progress_bar(ui, pct / 100.0, 6.0);
                    });
                    if widgets::button(ui, None, "Cancelar", Kind::Soft, 36.0, true).clicked() {
                        self.queue.cancel();
                    }
                });
            });
    }
}

/// Search box with icon and focus glow; fills the width left in the current layout.
pub(crate) fn search_field(ui: &mut Ui, query: &mut String) {
    let field = ui.add(
        TextEdit::singleline(query)
            .font(FontId::proportional(14.0))
            .hint_text("Buscar app…")
            .desired_width(ui.available_width())
            .margin(Margin { left: 40, right: 14, top: 12, bottom: 13 }),
    );
    if field.has_focus() {
        let glow = Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 41);
        ui.painter().rect_stroke(field.rect.expand(2.0), CornerRadius::same(12), Stroke::new(4.0, glow), egui::StrokeKind::Outside);
    }
    let c = Pos2::new(field.rect.left() + 14.0 + 8.0, field.rect.center().y);
    widgets::icon(ui, Rect::from_center_size(c, Vec2::splat(16.0)), Icon::Search, FAINT, 2.2);
}

pub(crate) fn segment(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let color = if on { TEXT } else { FAINT };
    let galley = ui.painter().layout_no_wrap(label.into(), FontId::new(13.0, semibold()), color);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 28.0, 34.0), Sense::click());
    if on {
        ui.painter().rect_filled(rect, CornerRadius::same(8), Color32::from_rgb(0x2A, 0x2D, 0x33));
    }
    ui.painter().galley(Pos2::new(rect.left() + 14.0, rect.center().y - galley.size().y / 2.0), galley, color);
    resp.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// A child ui over the horizontal slice `x0..x1` of a row.
pub(crate) fn cell(ui: &mut Ui, row: Rect, x0: f32, x1: f32, layout: Layout, add: impl FnOnce(&mut Ui)) {
    let r = Rect::from_min_max(Pos2::new(x0, row.top()), Pos2::new(x1, row.bottom()));
    let mut c = ui.new_child(UiBuilder::new().id_salt(x0.to_bits()).max_rect(r).layout(layout));
    add(&mut c);
}

/// One 68px row. Returns true when its own "Actualizar"/"Reintentar" was clicked.
fn row(ui: &mut Ui, app: &App, status: Status, error: Option<&str>, selected: &mut bool, enabled: bool) -> bool {
    // Per-app id scope: cells of different rows must not share widget ids.
    ui.push_id(&app.id, |ui| row_inner(ui, app, status, error, selected, enabled)).inner
}

fn row_inner(ui: &mut Ui, app: &App, status: Status, error: Option<&str>, selected: &mut bool, enabled: bool) -> bool {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::hover());
    // Not `resp.hovered()`: that goes false while the pointer is over the row's own buttons.
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 0, Color32::from_white_alpha(6));
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, Color32::from_white_alpha(10)));
    let (left, right) = (rect.left() + 18.0, rect.right() - 18.0);
    let ltr = Layout::left_to_right(Align::Center);
    let rtl = Layout::right_to_left(Align::Center);
    let mut clicked = false;

    cell(ui, rect, left, left + 18.0, ltr, |ui| {
        ui.add_enabled(status == Status::Available, Checkbox::new(selected, ""));
    });
    cell(ui, rect, left + 32.0, left + 72.0, ltr, |ui| widgets::tile(ui, app));
    cell(ui, rect, left + 86.0, left + 296.0, Layout::top_down(Align::Min), |ui| {
        ui.add_space(15.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add(egui::Label::new(RichText::new(&app.name).size(14.5).family(semibold())).truncate());
        ui.add(egui::Label::new(RichText::new(&app.vendor).size(12.5).color(FAINT)).truncate());
    });

    let action_x = right - 150.0;
    let size_x = action_x - 14.0 - 64.0;
    let chip_x = size_x - 14.0 - 86.0;
    cell(ui, rect, left + 310.0, chip_x - 14.0, ltr, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(&app.from).monospace().size(13.0).color(FAINT));
        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
        widgets::icon(ui, r, Icon::Arrow, FAINT, 2.2);
        ui.label(RichText::new(&app.to).monospace().size(13.0).color(ACCENT_TEXT));
    });
    cell(ui, rect, chip_x, chip_x + 86.0, ltr, |ui| widgets::chip(ui, app.source.label()));
    cell(ui, rect, size_x, size_x + 64.0, rtl, |ui| {
        ui.label(RichText::new(model::format_size(app.size_mb)).monospace().size(12.5).color(FAINT));
    });
    cell(ui, rect, action_x, right, rtl, |ui| match status {
        Status::Available => clicked = widgets::button(ui, None, "Actualizar", Kind::Soft, 34.0, enabled).clicked(),
        Status::Failed => {
            let tip = error.unwrap_or("Error desconocido").to_string();
            clicked = widgets::button(ui, None, "Reintentar", Kind::Soft, 34.0, enabled).on_hover_text(tip).clicked();
        }
        Status::Queued => {
            ui.label(RichText::new("En cola").size(13.0).color(FAINT));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            widgets::icon(ui, r, Icon::Clock, FAINT, 2.2);
        }
        Status::Updating(p) => {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                // Content is ~26px tall in a 68px row: push it to the vertical center.
                ui.add_space(21.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    let phase = if p < 70.0 { "Descargando" } else { "Instalando" };
                    ui.label(RichText::new(phase).size(12.0).color(TEXT_2));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}%", p.round())).monospace().size(12.0).color(ACCENT_TEXT));
                    });
                });
                widgets::progress_bar(ui, p / 100.0, 5.0);
            });
        }
        Status::Done => {
            ui.label(RichText::new("Actualizada").size(13.0).family(semibold()).color(OK));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            widgets::icon(ui, r, Icon::Check, OK, 2.6);
        }
    });
    clicked
}
