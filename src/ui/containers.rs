//! The Docker and Colima card of the Storage page: what runs, what is stopped or unused, and how
//! old it is, so the user decides what to delete.
use std::time::SystemTime;

use eframe::egui::{self, Align, Frame, Layout, Margin, RichText, Sense, Ui, Vec2};

use super::disk::Ask;
use crate::docker::{Action, Usage};
use crate::i18n::{t, tf, tf1};
use crate::model;
use crate::pages::{card, list_row};
use crate::storage::format_bytes;
use crate::theme::*;
use crate::ui::{UpdatesApp, cell};
use crate::widgets::{self, Icon, Kind};

/// One line of the card.
struct Row {
    title: String,
    chip: Option<String>,
    sub: String,
    indent: f32,
    size: Option<u64>,
    /// Button text, what it does, and the question asked before doing it.
    action: Option<(String, Action, String)>,
}

enum Click {
    Ask(Action),
    Go(Action),
    Cancel,
}

fn ago(t: Option<SystemTime>) -> String {
    t.map_or("—".to_string(), |t| model::ago_short(t, SystemTime::now()))
}

fn draw(ui: &mut Ui, ask: &Option<Ask>, busy: bool, row: Row) -> Option<Click> {
    let (rect, left, right) = list_row(ui);
    let asked = row.action.as_ref().is_some_and(|(_, a, _)| matches!(ask, Some(Ask::Docker(x)) if x == a));
    let left = left + row.indent;
    let mut click = None;
    cell(ui, rect, left, right - 290.0, Layout::top_down(Align::Min), |ui| {
        ui.add_space(15.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            ui.add(egui::Label::new(RichText::new(&row.title).size(14.5).family(semibold())).truncate());
            if let Some(chip) = &row.chip {
                widgets::chip(ui, chip);
            }
        });
        match (asked, &row.action) {
            (true, Some((_, _, question))) => ui.add(egui::Label::new(RichText::new(question).size(FONT_SIZE_MICRO).family(semibold())).truncate()),
            _ => ui.add(egui::Label::new(RichText::new(&row.sub).size(FONT_SIZE_MICRO).color(faint())).truncate()),
        };
    });
    if let Some(bytes) = row.size {
        cell(ui, rect, right - 280.0, right - 196.0, Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(format_bytes(bytes)).monospace().size(FONT_SIZE_SMALL).color(text_2()));
        });
    }
    if let Some((label, action, _)) = &row.action {
        cell(ui, rect, right - 190.0, right, Layout::right_to_left(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            if asked {
                if widgets::button(ui, None, t("Eliminar"), Kind::Primary, BUTTON_HEIGHT_SM, true).clicked() {
                    click = Some(Click::Go(action.clone()));
                }
                if widgets::button(ui, None, t("Cancelar"), Kind::Ghost, BUTTON_HEIGHT_SM, true).clicked() {
                    click = Some(Click::Cancel);
                }
            } else if widgets::button(ui, None, label, Kind::Soft, BUTTON_HEIGHT_SM, !busy).clicked() {
                click = Some(Click::Ask(action.clone()));
            }
        });
    }
    click
}

impl UpdatesApp {
    pub(super) fn containers_card(&mut self, ui: &mut Ui) {
        if self.storage.engines.is_empty() {
            return;
        }
        let engines = self.storage.engines.clone();
        let busy = self.storage.acting;
        let mut click = None;
        card(ui, |ui| {
            Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(t("Docker y Colima")).size(15.0).family(semibold()));
            });
            for e in &engines {
                let context = e.context.clone();
                let unused: Vec<_> = e.images.iter().filter(|i| i.usage == Usage::Unused).collect();
                let vm_running = e.vm.as_ref().is_none_or(|v| v.running);
                let sub = match &e.vm {
                    Some(v) if !v.running => format!("{} · {}", tf1("Última vez usada: {0}", &ago(v.last_used)), tf1("Disco de la VM: {0}", &format_bytes(v.disk_bytes))),
                    Some(v) => tf1("Disco de la VM: {0}", &format_bytes(v.disk_bytes)),
                    None => String::new(),
                };
                let action = match &e.vm {
                    Some(v) if !v.running => Some((t("Eliminar VM").to_string(), Action::Vm { name: v.name.clone() }, tf1("Se borra la VM «{0}» con todo lo que tiene dentro. No se puede deshacer.", &v.name))),
                    _ if !unused.is_empty() => {
                        let ids = unused.iter().map(|i| i.id.clone()).collect();
                        let total: u64 = unused.iter().map(|i| i.bytes).sum();
                        Some((tf1("Eliminar sin usar ({0})", &unused.len().to_string()), Action::Images { context: context.clone(), ids }, tf("¿Eliminar {0} imágenes sin usar ({1})?", &[unused.len().to_string(), format_bytes(total)])))
                    }
                    _ => None,
                };
                let chip = if vm_running { t("En ejecución") } else { t("Apagada") };
                click = click.take().or(draw(ui, &self.storage.ask, busy, Row { title: e.label.clone(), chip: Some(chip.into()), sub, indent: 0.0, size: e.vm.as_ref().map(|v| v.disk_bytes), action }));
                for i in &e.images {
                    let (state, what) = match i.usage {
                        Usage::Running => (t("En uso"), None),
                        Usage::Stopped => (t("Solo la usa un contenedor detenido"), None),
                        Usage::Unused => (t("Sin usar"), Some((t("Eliminar").to_string(), Action::Images { context: context.clone(), ids: vec![i.id.clone()] }, t("¿Eliminar esta imagen?").to_string()))),
                    };
                    let sub = format!("{} · {}", tf1("Creada: {0}", &ago(i.created)), state);
                    click = click.take().or(draw(ui, &self.storage.ask, busy, Row { title: i.name.clone(), chip: None, sub, indent: 28.0, size: Some(i.bytes), action: what }));
                }
                for c in &e.stopped {
                    let sub = format!("{} · {}", t("Contenedor detenido"), tf1("Creado: {0}", &ago(c.created)));
                    let sub = if c.image.is_empty() { sub } else { format!("{sub} · {}", c.image) };
                    let what = Some((t("Eliminar").to_string(), Action::Container { context: context.clone(), id: c.id.clone() }, t("¿Eliminar este contenedor?").to_string()));
                    click = click.take().or(draw(ui, &self.storage.ask, busy, Row { title: c.name.clone(), chip: None, sub, indent: 28.0, size: None, action: what }));
                }
                if e.cache_bytes > 0 {
                    let what = Some((t("Limpiar").to_string(), Action::BuildCache { context: context.clone() }, t("¿Limpiar la caché de compilación?").to_string()));
                    click = click.take().or(draw(ui, &self.storage.ask, busy, Row { title: t("Caché de compilación").into(), chip: None, sub: t("Se vuelve a generar al compilar de nuevo").into(), indent: 28.0, size: Some(e.cache_bytes), action: what }));
                }
            }
            Frame::new().inner_margin(Margin::symmetric(18, 12)).show(ui, |ui| {
                ui.set_width(ui.available_width());
                if self.storage.acting && self.storage.note_docker {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
                        widgets::icon(ui, r, Icon::Busy, faint(), 2.2);
                        ui.label(RichText::new(t("Eliminando… puede tardar unos minutos")).size(FONT_SIZE_SMALL).color(faint()));
                    });
                } else if let Some(note) = self.storage.note.as_ref().filter(|_| self.storage.note_docker) {
                    ui.label(RichText::new(note).size(FONT_SIZE_SMALL).color(ok()));
                }
                if engines.iter().any(|e| e.vm.is_some()) {
                    ui.label(RichText::new(t("Tras borrar, el disco de la VM se compacta para devolver el espacio al equipo.")).size(FONT_SIZE_MICRO).color(faint()));
                }
            });
        });
        ui.add_space(16.0);
        match click {
            Some(Click::Ask(a)) => self.storage.ask = Some(Ask::Docker(a)),
            Some(Click::Cancel) => self.storage.ask = None,
            Some(Click::Go(a)) => self.storage_act(true, move || match crate::docker::run(&a) {
                Ok(_) => t("Listo").to_string(),
                Err(e) => tf1("No se pudo eliminar: {0}", &e),
            }),
            None => {}
        }
    }
}
