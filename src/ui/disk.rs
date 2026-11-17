//! Storage page: how full the disk is, what can be cleaned, and what takes the most room.
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

use eframe::egui::{self, Align, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea, Sense, Ui, Vec2};

use crate::i18n::{t, tf, tf1};
use crate::pages::{card, header, list_row};
use crate::docker::{self, Action, Engine};
use crate::storage::{self, Disk, Item, Place, format_bytes};
use crate::theme::*;
use crate::ui::{UpdatesApp, cell};
use crate::widgets::{self, Icon, Kind};

/// What the background threads report.
enum Msg {
    Overview(Option<Disk>, Vec<Place>),
    /// A folder's entries, before the folders among them are measured.
    Folder(PathBuf, Vec<Item>),
    Size(PathBuf, PathBuf, u64),
    FolderDone(PathBuf),
    Docker(Vec<Engine>),
    Done(String),
}

/// A destructive action waiting for the user's yes.
pub(super) enum Ask {
    Clean,
    Trash(PathBuf),
    Docker(Action),
}

pub(crate) struct Storage {
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    started: bool,
    disk: Option<Disk>,
    places: Vec<Place>,
    picked: HashSet<usize>,
    /// The folder being explored (the home folder to begin with).
    dir: Option<PathBuf>,
    items: Vec<Item>,
    loading_overview: bool,
    loading_folder: bool,
    pub(super) acting: bool,
    pub(super) ask: Option<Ask>,
    pub(super) note: Option<String>,
    /// Whether `note` is about Docker (shown in its card) or about the cleanup.
    pub(super) note_docker: bool,
    pub(super) engines: Vec<Engine>,
    pub(super) loading_docker: bool,
}

impl Storage {
    pub(crate) fn new() -> Self {
        let (tx, rx) = mpsc::channel();
        Self { tx, rx, started: false, disk: None, places: Vec::new(), picked: HashSet::new(), dir: None, items: Vec::new(), loading_overview: false, loading_folder: false, acting: false, ask: None, note: None, note_docker: false, engines: Vec::new(), loading_docker: false }
    }
}

/// Biggest first; the folders still being measured wait at the bottom.
fn sort_items(items: &mut [Item]) {
    items.sort_by_key(|i| (i.bytes.is_none(), std::cmp::Reverse(i.bytes)));
}

/// Lists a folder and measures its subfolders, reporting each step.
fn scan_folder(dir: &Path, tx: &Sender<Msg>, ctx: &egui::Context) {
    let items = storage::list(dir);
    let _ = tx.send(Msg::Folder(dir.to_path_buf(), items.clone()));
    ctx.request_repaint();
    storage::measure(&items, |path, bytes| {
        let _ = tx.send(Msg::Size(dir.to_path_buf(), path.to_path_buf(), bytes));
        ctx.request_repaint();
    });
    let _ = tx.send(Msg::FolderDone(dir.to_path_buf()));
    ctx.request_repaint();
}

fn section(ui: &mut Ui, title: &str, add_right: impl FnOnce(&mut Ui)) {
    Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new(title).size(15.0).family(semibold()));
            ui.with_layout(Layout::right_to_left(Align::Center), add_right);
        });
    });
}

fn busy_label(ui: &mut Ui, text: &str) {
    ui.horizontal(|ui| {
        ui.add_space(18.0);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
        widgets::icon(ui, r, Icon::Busy, faint(), 2.2);
        ui.label(RichText::new(text).size(FONT_SIZE_SMALL).color(faint()));
    });
    ui.add_space(14.0);
}

impl UpdatesApp {
    pub(crate) fn storage_page(&mut self, ui: &mut Ui) {
        if !self.storage.started {
            self.storage.started = true;
            if let Some(home) = storage::home() {
                self.storage_refresh(home);
            }
        }
        self.storage_poll();
        let subtitle = match &self.storage.disk {
            Some(d) => tf("{0} libres de {1}", &[format_bytes(d.free), format_bytes(d.total)]),
            None => t("Calculando…").to_string(),
        };
        header(ui, t("Almacenamiento"), &subtitle);
        ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            self.disk_card(ui);
            ui.add_space(16.0);
            self.cleanup_card(ui);
            ui.add_space(16.0);
            self.containers_card(ui);
            self.explorer_card(ui);
            ui.add_space(8.0);
        });
    }

    fn storage_poll(&mut self) {
        while let Ok(msg) = self.storage.rx.try_recv() {
            let s = &mut self.storage;
            match msg {
                Msg::Overview(disk, places) => {
                    s.disk = disk;
                    // Everything is picked but the trash: emptying it cannot be undone.
                    s.picked = places.iter().enumerate().filter(|(_, p)| p.label != "Papelera").map(|(i, _)| i).collect();
                    s.places = places;
                    s.loading_overview = false;
                }
                Msg::Folder(dir, items) if s.dir.as_deref() == Some(dir.as_path()) => {
                    s.items = items;
                    s.loading_folder = true;
                    sort_items(&mut s.items);
                }
                Msg::Size(dir, path, bytes) if s.dir.as_deref() == Some(dir.as_path()) => {
                    if let Some(item) = s.items.iter_mut().find(|i| i.path == path) {
                        item.bytes = Some(bytes);
                    }
                    sort_items(&mut s.items);
                }
                Msg::FolderDone(dir) if s.dir.as_deref() == Some(dir.as_path()) => s.loading_folder = false,
                // Late news of a folder the user already left.
                Msg::Folder(..) | Msg::Size(..) | Msg::FolderDone(..) => {}
                Msg::Docker(engines) => {
                    s.engines = engines;
                    s.loading_docker = false;
                }
                Msg::Done(note) => {
                    s.note = Some(note);
                    s.acting = false;
                }
            }
        }
    }

    /// Measures the disk and the cleanable places, and opens `dir`.
    fn storage_refresh(&mut self, dir: PathBuf) {
        self.storage.loading_overview = true;
        let (tx, ctx) = (self.storage.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let disk = storage::home().and_then(|h| storage::disk(&h));
            let _ = tx.send(Msg::Overview(disk, storage::cleanable()));
            ctx.request_repaint();
        });
        self.storage.loading_docker = true;
        let (tx, ctx) = (self.storage.tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            let _ = tx.send(Msg::Docker(docker::scan()));
            ctx.request_repaint();
        });
        self.storage_open(dir);
    }

    fn storage_open(&mut self, dir: PathBuf) {
        let s = &mut self.storage;
        (s.dir, s.loading_folder, s.ask) = (Some(dir.clone()), true, None);
        s.items.clear();
        let (tx, ctx) = (s.tx.clone(), self.ctx.clone());
        thread::spawn(move || scan_folder(&dir, &tx, &ctx));
    }

    /// Runs `work` on a thread, then re-measures everything and reports its note.
    pub(super) fn storage_act(&mut self, docker: bool, work: impl FnOnce() -> String + Send + 'static) {
        let s = &mut self.storage;
        (s.acting, s.ask, s.note, s.note_docker) = (true, None, None, docker);
        let (tx, ctx, dir) = (s.tx.clone(), self.ctx.clone(), s.dir.clone());
        thread::spawn(move || {
            let note = work();
            let disk = storage::home().and_then(|h| storage::disk(&h));
            let _ = tx.send(Msg::Overview(disk, storage::cleanable()));
            let _ = tx.send(Msg::Done(note));
            if docker {
                let _ = tx.send(Msg::Docker(crate::docker::scan()));
            }
            ctx.request_repaint();
            // The sizes changed: read the folder again, without holding the note back.
            if let Some(dir) = dir {
                scan_folder(&dir, &tx, &ctx);
            }
        });
    }

    fn disk_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            Frame::new().inner_margin(18).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 12.0;
                let (bar, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 16.0), Sense::hover());
                ui.painter().rect_filled(bar, 8, tint(10));
                let (mut other_used, mut cleanable) = (0, self.storage.places.iter().map(|p| p.bytes).sum::<u64>());
                if let Some(d) = &self.storage.disk
                    && d.total > 0
                {
                    let used = d.total.saturating_sub(d.free);
                    cleanable = cleanable.min(used);
                    other_used = used - cleanable;
                    let x = |bytes: u64| bar.left() + bar.width() * (bytes as f32 / d.total as f32);
                    let first = Rect::from_min_max(bar.min, egui::pos2(x(other_used), bar.bottom()));
                    ui.painter().rect_filled(first, CornerRadius { nw: 8, sw: 8, ne: 0, se: 0 }, tint(45));
                    let second = Rect::from_min_max(egui::pos2(x(other_used), bar.top()), egui::pos2(x(used), bar.bottom()));
                    ui.painter().rect_filled(second, 0, ACCENT);
                }
                let free = self.storage.disk.as_ref().map_or(0, |d| d.free);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing.x = 22.0;
                    for (color, label, bytes) in [(tint(45), t("Otros archivos"), other_used), (ACCENT, t("Se puede limpiar"), cleanable), (tint(10), t("Libre"), free)] {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 6.0;
                            let (r, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                            ui.painter().rect_filled(r, 3, color);
                            ui.label(RichText::new(label).size(FONT_SIZE_SMALL).color(muted()));
                            ui.label(RichText::new(format_bytes(bytes)).size(FONT_SIZE_SMALL).family(semibold()));
                        });
                    }
                });
            });
        });
    }

    fn cleanup_card(&mut self, ui: &mut Ui) {
        card(ui, |ui| {
            section(ui, t("Se puede limpiar"), |_| {});
            if self.storage.loading_overview && self.storage.places.is_empty() {
                busy_label(ui, t("Calculando…"));
                return;
            }
            if self.storage.places.is_empty() {
                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(RichText::new(t("Nada que limpiar por ahora")).size(FONT_SIZE_SMALL).color(faint()));
                });
                ui.add_space(18.0);
            }
            for i in 0..self.storage.places.len() {
                let (label, hint, bytes) = {
                    let p = &self.storage.places[i];
                    (p.label, p.hint, p.bytes)
                };
                let (rect, left, right) = list_row(ui);
                let mut on = self.storage.picked.contains(&i);
                cell(ui, rect, left, left + 26.0, Layout::left_to_right(Align::Center), |ui| {
                    ui.checkbox(&mut on, "");
                });
                cell(ui, rect, left + 36.0, right - 110.0, Layout::top_down(Align::Min), |ui| {
                    ui.add_space(15.0);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    ui.add(egui::Label::new(RichText::new(t(label)).size(14.5).family(semibold())).truncate());
                    ui.add(egui::Label::new(RichText::new(t(hint)).size(FONT_SIZE_MICRO).color(faint())).truncate());
                });
                cell(ui, rect, right - 100.0, right, Layout::right_to_left(Align::Center), |ui| {
                    ui.label(RichText::new(format_bytes(bytes)).monospace().size(FONT_SIZE_SMALL).color(text_2()));
                });
                if on {
                    self.storage.picked.insert(i);
                } else {
                    self.storage.picked.remove(&i);
                }
            }
            self.cleanup_footer(ui);
        });
    }

    fn cleanup_footer(&mut self, ui: &mut Ui) {
        let chosen: Vec<Place> = self.storage.places.iter().enumerate().filter(|(i, _)| self.storage.picked.contains(i)).map(|(_, p)| p.clone()).collect();
        let total: u64 = chosen.iter().map(|p| p.bytes).sum();
        let (mut ask, mut go, mut cancel) = (false, false, false);
        Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if self.storage.acting {
                    let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                    widgets::icon(ui, r, Icon::Busy, faint(), 2.2);
                    ui.label(RichText::new(t("Limpiando…")).size(FONT_SIZE_SMALL).color(faint()));
                } else if matches!(self.storage.ask, Some(Ask::Clean)) {
                    ui.label(RichText::new(tf1("¿Borrar {0}? No se puede deshacer.", &format_bytes(total))).size(FONT_SIZE_SMALL).family(semibold()));
                } else if let Some(note) = self.storage.note.as_ref().filter(|_| !self.storage.note_docker) {
                    ui.add(egui::Label::new(RichText::new(note).size(FONT_SIZE_SMALL).color(ok())).truncate());
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if matches!(self.storage.ask, Some(Ask::Clean)) {
                        go = widgets::button(ui, None, t("Borrar"), Kind::Primary, BUTTON_HEIGHT_SM, true).clicked();
                        cancel = widgets::button(ui, None, t("Cancelar"), Kind::Ghost, BUTTON_HEIGHT_SM, true).clicked();
                    } else {
                        let label = tf1("Liberar {0}", &format_bytes(total));
                        ask = widgets::button(ui, None, &label, Kind::Primary, BUTTON_HEIGHT_SM, total > 0 && !self.storage.acting).clicked();
                    }
                });
            });
        });
        if ask {
            self.storage.ask = Some(Ask::Clean);
        }
        if cancel {
            self.storage.ask = None;
        }
        if go {
            self.storage_act(false, move || {
                let before: u64 = chosen.iter().map(|p| p.bytes).sum();
                let skipped: usize = chosen.iter().map(storage::clean).sum();
                let after: u64 = chosen.into_iter().map(|mut p| { storage::measure_place(&mut p); p.bytes }).sum();
                let freed = tf1("Se liberaron {0}", &format_bytes(before.saturating_sub(after)));
                if skipped > 0 { format!("{freed} · {}", tf1("{0} elementos estaban en uso y se dejaron", &skipped.to_string())) } else { freed }
            });
        }
    }

    fn explorer_card(&mut self, ui: &mut Ui) {
        let Some(dir) = self.storage.dir.clone() else { return };
        let home = storage::home().unwrap_or_default();
        let (mut open, mut reload, mut trash, mut confirmed, mut cancel) = (None, false, None, None, false);
        card(ui, |ui| {
            section(ui, t("Qué ocupa más espacio"), |ui| {
                reload = widgets::button(ui, Some(Icon::Refresh), t("Volver a calcular"), Kind::Ghost, BUTTON_HEIGHT_SM, !self.storage.loading_folder && !self.storage.acting).clicked();
            });
            Frame::new().inner_margin(Margin { left: 18, right: 18, top: 0, bottom: 12 }).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    if dir != home && widgets::button(ui, None, t("Subir"), Kind::Soft, BUTTON_HEIGHT_SM, true).clicked() {
                        open = dir.parent().map(Path::to_path_buf);
                    }
                    ui.add(egui::Label::new(RichText::new(dir.display().to_string()).monospace().size(FONT_SIZE_TINY).color(faint())).truncate());
                });
            });
            if self.storage.loading_folder {
                busy_label(ui, t("Calculando tamaños…"));
            }
            if !self.storage.loading_folder && self.storage.items.is_empty() {
                ui.horizontal(|ui| {
                    ui.add_space(18.0);
                    ui.label(RichText::new(t("Carpeta vacía")).size(FONT_SIZE_SMALL).color(faint()));
                });
                ui.add_space(18.0);
            }
            let biggest = self.storage.items.iter().filter_map(|i| i.bytes).max().unwrap_or(1).max(1);
            for item in &self.storage.items {
                let asked = matches!(&self.storage.ask, Some(Ask::Trash(p)) if *p == item.path);
                let (rect, left, right) = list_row(ui);
                let name_w = ((right - left) * 0.34).clamp(150.0, 260.0);
                let (actions_x, size_x) = (right - 176.0, right - 252.0);
                cell(ui, rect, left, left + name_w, Layout::top_down(Align::Min), |ui| {
                    ui.add_space(15.0);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    if item.is_dir {
                        let name = ui.add(egui::Label::new(RichText::new(&item.name).size(14.5).family(semibold()).color(accent_text())).truncate().sense(Sense::click()));
                        if name.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            open = Some(item.path.clone());
                        }
                    } else {
                        ui.add(egui::Label::new(RichText::new(&item.name).size(14.5).family(semibold())).truncate());
                    }
                    ui.label(RichText::new(if item.is_dir { t("Carpeta") } else { t("Archivo") }).size(FONT_SIZE_MICRO).color(faint()));
                });
                let bar_x = (left + name_w + 12.0, size_x - 12.0);
                cell(ui, rect, bar_x.0, bar_x.1, Layout::left_to_right(Align::Center), |ui| {
                    if asked {
                        ui.add(egui::Label::new(RichText::new(t("¿Mover a la papelera?")).size(FONT_SIZE_SMALL).family(semibold())).truncate());
                    } else if let Some(bytes) = item.bytes
                        && bar_x.1 - bar_x.0 > 30.0
                    {
                        let (r, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 8.0), Sense::hover());
                        ui.painter().rect_filled(r, 4, tint(10));
                        let fill = Rect::from_min_size(r.min, Vec2::new((r.width() * bytes as f32 / biggest as f32).max(3.0), r.height()));
                        ui.painter().rect_filled(fill, 4, ACCENT);
                    }
                });
                cell(ui, rect, size_x, actions_x - 8.0, Layout::right_to_left(Align::Center), |ui| {
                    let size = item.bytes.map_or("…".to_string(), format_bytes);
                    ui.label(RichText::new(size).monospace().size(FONT_SIZE_SMALL).color(text_2()));
                });
                cell(ui, rect, actions_x, right, Layout::right_to_left(Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    if asked {
                        if widgets::button(ui, None, t("Mover"), Kind::Primary, BUTTON_HEIGHT_SM, true).clicked() {
                            confirmed = Some(item.path.clone());
                        }
                        cancel |= widgets::button(ui, None, t("Cancelar"), Kind::Ghost, BUTTON_HEIGHT_SM, true).clicked();
                    } else {
                        let allowed = !self.storage.acting && storage::can_remove(&item.path, &home);
                        if widgets::button(ui, None, t("Papelera"), Kind::Soft, BUTTON_HEIGHT_SM, allowed).clicked() {
                            trash = Some(item.path.clone());
                        }
                        if widgets::button(ui, None, t("Mostrar"), Kind::Ghost, BUTTON_HEIGHT_SM, true).clicked() {
                            storage::reveal(&item.path);
                        }
                    }
                });
            }
        });
        if let Some(path) = open {
            self.storage_open(path);
        } else if reload {
            self.storage_refresh(dir);
        } else if let Some(path) = trash {
            self.storage.ask = Some(Ask::Trash(path));
        } else if cancel {
            self.storage.ask = None;
        } else if let Some(path) = confirmed {
            self.storage_act(false, move || {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match storage::trash(&path) {
                    Ok(()) => tf1("«{0}» está en la papelera", &name),
                    Err(e) => tf1("No se pudo mover a la papelera: {0}", &e),
                }
            });
        }
    }
}
