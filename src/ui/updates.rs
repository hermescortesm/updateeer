//! The Updates page: what is outdated, the queue running, and one row per app.
use super::*;

impl UpdatesApp {
    /// Indices into `apps` that pass the search and filters, in the chosen order.
    pub(super) fn visible(&self) -> Vec<usize> {
        let q = self.filter_state.query.trim().to_lowercase();
        let now = SystemTime::now();
        let days = |a: &App| a.installed_at.map(|t| model::days_ago(t, now));
        let mut rows: Vec<usize> = (0..self.apps.len())
            .filter(|&i| {
                let a = &self.apps[i];
                // System packages are tools unless they ship a launcher (a graphical app).
                let tool = a.id.starts_with("formula:") || (matches!(a.source, Source::Apt | Source::Pacman | Source::Aur | Source::Dnf | Source::Zypper | Source::Apk | Source::Xbps) && !crate::linuxpkg::is_gui(&a.id));
                (match self.filter_state.filter {
                    Filter::All => true,
                    Filter::Only(s) => a.source == s,
                }) && (match self.filter_state.kind {
                    Type::All => true,
                    Type::Apps => !tool,
                    Type::Tools => tool,
                }) && (match self.filter_state.age {
                    Age::Any => true,
                    Age::Week => days(a).is_some_and(|d| d <= 7),
                    Age::Month => days(a).is_some_and(|d| d <= 30),
                    Age::Old => days(a).is_some_and(|d| d > 90),
                }) && (q.is_empty() || a.name.to_lowercase().contains(&q))
            })
            .collect();
        // `apps` is already sorted by name; unknown dates always go last.
        match self.filter_state.sort {
            Sort::Name => {}
            Sort::Newest => rows.sort_by_key(|&i| (self.apps[i].installed_at.is_none(), std::cmp::Reverse(self.apps[i].installed_at))),
            Sort::Oldest => rows.sort_by_key(|&i| (self.apps[i].installed_at.is_none(), self.apps[i].installed_at)),
        }
        // Updated apps sink to the bottom, so it is easy to see which ones are done.
        rows.sort_by_key(|&i| self.queue.status(&self.apps[i].id) == Status::Done);
        rows
    }

    pub(super) fn filters_active(&self) -> bool {
        (self.filter_state.kind, self.filter_state.age, self.filter_state.sort) != (Type::All, Age::Any, Sort::Name)
    }

    /// The Updates page: summary, actions, filters and the list.
    pub(super) fn updates_page(&mut self, ui: &mut Ui) {
        let available = self.queue.available_count();
        let total_mb: u32 = self
            .apps
            .iter()
            .filter(|a| self.queue.status(&a.id) == Status::Available)
            .filter_map(|a| a.size_mb)
            .sum();
        let summary = summary(&Counts {
            scanning: self.scanning(),
            error: self.scan.error.as_deref(),
            warning: self.scan.warning.as_deref(),
            available,
            total_mb,
            busy: self.queue.is_busy(),
            failed: self.queue.failed_count(),
        });
        // No actions while a scan runs: its result replaces the queue.
        let can_act = !self.scanning();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new(t("Actualizaciones")).size(FONT_SIZE_DISPLAY).family(display()));
                ui.label(RichText::new(summary).size(FONT_SIZE_BODY).color(muted()));
            });
            ui.with_layout(Layout::right_to_left(Align::Max), |ui| {
                ui.spacing_mut().item_spacing.x = 16.0;
                if widgets::button(ui, Some(Icon::Download), t("Actualizar todo"), Kind::Primary, BUTTON_HEIGHT_LG, available > 0 && can_act).clicked() {
                    let ids: Vec<String> = self.queue.available().map(String::from).collect();
                    self.queue.enqueue(ids.iter().map(String::as_str));
                    self.selected.clear();
                }
                let idle = !self.scanning() && !self.queue.is_busy();
                let (ic, label) = if self.scanning() { (Icon::Busy, t("Buscando…")) } else { (Icon::Refresh, t("Buscar actualizaciones")) };
                if widgets::button(ui, Some(ic), label, Kind::Ghost, BUTTON_HEIGHT_LG, idle).clicked() {
                    self.start_scan();
                }
            });
        });
        ui.add_space(SPACING_LG);

        if self.queue.is_busy() {
            self.batch_banner(ui);
            ui.add_space(SPACING_LG);
        }
        if self.permission_banner(ui) {
            ui.add_space(SPACING_LG);
        }
        if self.setup_banner(ui) {
            ui.add_space(SPACING_LG);
        }
        if self.release_banner(ui) {
            ui.add_space(SPACING_LG);
        }

        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                Frame::new()
                    .fill(field())
                    .stroke(Stroke::new(1.0, tint(20)))
                    .corner_radius(12)
                    .inner_margin(4)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.horizontal(|ui| {
                            for (label, f) in source_filters() {
                                if segment(ui, label, self.filter_state.filter == f).clicked() {
                                    self.filter_state.filter = f;
                                }
                            }
                        });
                    });
                search_field(ui, &mut self.filter_state.query);
            });
        });
        ui.add_space(SPACING_MD);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            combo(ui, t("Tipo"), &mut self.filter_state.kind, &[(Type::All, t("Todo")), (Type::Apps, t("Apps")), (Type::Tools, t("Herramientas"))]);
            ui.add_space(SPACING_SM);
            combo(ui, t("Instalada"), &mut self.filter_state.age, &[(Age::Any, t("Cualquiera")), (Age::Week, t("Últimos 7 días")), (Age::Month, t("Últimos 30 días")), (Age::Old, t("Hace más de 3 meses"))]);
            ui.add_space(SPACING_SM);
            combo(ui, t("Orden"), &mut self.filter_state.sort, &[(Sort::Name, t("Nombre")), (Sort::Newest, t("Más recientes")), (Sort::Oldest, t("Más antiguas"))]);
            if self.filters_active() && widgets::button(ui, None, t("Limpiar"), Kind::Soft, BUTTON_HEIGHT_SM, true).clicked() {
                (self.filter_state.kind, self.filter_state.age, self.filter_state.sort) = (Type::All, Age::Any, Sort::Name);
            }
        });
        ui.add_space(SPACING_LG);

        let rows = self.visible();
        let visible_ids: Vec<String> = rows
            .iter()
            .map(|&i| &self.apps[i])
            .filter(|a| self.queue.status(&a.id) == Status::Available)
            .map(|a| a.id.clone())
            .collect();
        let mut to_update: Vec<String> = Vec::new();
        let mut open: Option<App> = None;
        let mut retry = false;
        Frame::new()
            .fill(panel())
            .stroke(Stroke::new(1.0, line()))
            .corner_radius(16)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.set_height(ui.available_height());
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                let selected_visible: Vec<&String> = visible_ids.iter().filter(|id| self.selected.contains(*id)).collect();
                let n = selected_visible.len();
                let all_on = !visible_ids.is_empty() && n == visible_ids.len();
                let (head, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 52.0), Sense::hover());
                ui.painter().hline(head.x_range(), head.bottom() - 0.5, Stroke::new(1.0, line()));
                let mut inner = ui.new_child(UiBuilder::new().max_rect(head.shrink2(Vec2::new(ROW_PADDING_X, 0.0))).layout(Layout::left_to_right(Align::Center)));
                let mut on = all_on;
                if inner.add_enabled(!visible_ids.is_empty(), Checkbox::new(&mut on, "")).changed() {
                    for id in &visible_ids {
                        if on { self.selected.insert(id.clone()); } else { self.selected.remove(id); }
                    }
                }
                let label = match n {
                    0 => t("Seleccionar todas").to_string(),
                    1 => t("1 seleccionada").to_string(),
                    n => tf1("{0} seleccionadas", &n.to_string()),
                };
                inner.label(RichText::new(label).size(FONT_SIZE_SMALL).family(semibold()).color(text_2()));
                inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, None, t("Actualizar seleccionadas"), Kind::Primary, BUTTON_HEIGHT_SM, n > 0 && can_act).clicked() {
                        to_update.extend(visible_ids.iter().filter(|id| self.selected.contains(*id)).cloned());
                    }
                });
                if self.scanning() {
                    widgets::busy_bar(ui, 3.0);
                }
                column_titles(ui);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if rows.is_empty() {
                        ui.add_space(64.0);
                        if self.offline() && !self.scanning() {
                            ui.vertical_centered(|ui| {
                                let (r, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
                                widgets::icon(ui, r, Icon::Offline, muted(), 2.0);
                                ui.add_space(SPACING_SM);
                                ui.label(RichText::new(Fail::Offline.text()).size(16.0).family(semibold()));
                                ui.label(RichText::new(t("Revisa tu Wi-Fi o cable y vuelve a intentarlo.")).size(FONT_SIZE_SMALL).color(faint()));
                                ui.add_space(SPACING_LG);
                                retry = widgets::button(ui, Some(Icon::Refresh), t("Reintentar"), Kind::Soft, BUTTON_HEIGHT_SM, true).clicked();
                            });
                        } else {
                        ui.vertical_centered(|ui| {
                            let (msg, color) = match (&self.scan.error, self.scanning(), self.apps.is_empty()) {
                                (Some(e), false, _) => (tf1("No se pudo buscar: {0}", &e.to_string()), muted()),
                                (_, true, _) => (t("Buscando…").into(), faint()),
                                (_, _, true) => (t("No hay actualizaciones.").into(), faint()),
                                _ => (t("Ninguna app coincide con tu búsqueda.").into(), faint()),
                            };
                            ui.label(RichText::new(msg).color(color));
                        });
                        }
                    }
                    for &i in &rows {
                        let app = &self.apps[i];
                        let status = self.queue.status(&app.id);
                        let mut sel = self.selected.contains(&app.id);
                        let was = sel;
                        let err = self.queue.error(&app.id);
                        let (update, show) = row(ui, app, status, err, &mut sel, can_act, self.icons.get(&app.id));
                        if update {
                            to_update.push(app.id.clone());
                        }
                        if show {
                            open = Some(app.clone());
                        }
                        if sel != was {
                            if sel { self.selected.insert(app.id.clone()); } else { self.selected.remove(&app.id); }
                        }
                    }
                });
            });
        if let Some(app) = open {
            self.open_detail(app, Page::Updates);
        }
        if retry {
            self.start_scan();
        }
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
            .fill(banner_bg())
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 77)))
            .corner_radius(BANNER_CORNER_RADIUS)
            .inner_margin(Margin::symmetric(BANNER_INNER_MARGIN_X, BANNER_INNER_MARGIN_Y))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 18.0;
                    let (dot, _) = ui.allocate_exact_size(Vec2::splat(10.0), Sense::hover());
                    ui.painter().circle_filled(dot.center(), 5.0, ACCENT);
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 110.0);
                        ui.spacing_mut().item_spacing.y = SPACING_SM;
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(tf("Actualizadas {0} de {1}", &[done.to_string(), total.to_string()])).size(FONT_SIZE_BODY).family(semibold()));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                ui.label(RichText::new(format!("{}%", pct.round())).monospace().color(accent_text()));
                            });
                        });
                        widgets::progress_bar(ui, pct / 100.0, 6.0);
                    });
                    if widgets::button(ui, None, t("Cancelar"), Kind::Soft, BUTTON_HEIGHT_MD, true).clicked() {
                        self.queue.cancel();
                        scan::cancel_running();
                    }
                });
            });
    }
}

/// What the Updates page knows when it writes the line under its title.
struct Counts<'a> {
    scanning: bool,
    error: Option<&'a str>,
    /// A source failed while others worked.
    warning: Option<&'a str>,
    available: usize,
    total_mb: u32,
    busy: bool,
    failed: usize,
}

/// The line under the "Updates" title: the most important state wins.
fn summary(c: &Counts) -> String {
    let main = if c.scanning {
        t("Buscando actualizaciones…").to_string()
    } else if let Some(e) = c.error {
        if Fail::Offline.is(e) { e.to_string() } else { tf1("No se pudo buscar: {0}", e) }
    } else if c.available > 0 {
        let size = if c.total_mb > 0 { format!(" · {}", model::format_size(Some(c.total_mb))) } else { String::new() };
        tf("{0} disponibles{1}", &[c.available.to_string(), size])
    } else if c.busy {
        t("Actualizando…").to_string()
    } else if c.failed > 0 {
        tf1("{0} fallidas", &c.failed.to_string())
    } else if c.warning.is_some() {
        t("Revisión incompleta").to_string()
    } else {
        t("Todo al día").to_string()
    };
    match (c.warning, c.scanning) {
        (Some(w), false) => format!("{main} · {w}"),
        _ => main,
    }
}

/// Horizontal anchors shared by the rows and the column titles.
struct Cols {
    left: f32,
    right: f32,
    chip_x: f32,
    size_x: f32,
    action_x: f32,
}

impl Cols {
    fn of(rect: Rect) -> Self {
        let (left, right) = (rect.left() + ROW_PADDING_X, rect.right() - ROW_PADDING_X);
        let action_x = right - ROW_ACTION_WIDTH;
        let size_x = action_x - ROW_VERSION_GAP - ROW_SIZE_WIDTH;
        let chip_x = size_x - ROW_VERSION_GAP - ROW_CHIP_WIDTH;
        Self { left, right, chip_x, size_x, action_x }
    }
}

/// Titles above the columns of the updates table, aligned with `row_inner`.
fn column_titles(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), COLUMN_TITLES_HEIGHT), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, line()));
    let Cols { left, chip_x, size_x, .. } = Cols::of(rect);
    let (ltr, rtl) = (Layout::left_to_right(Align::Center), Layout::right_to_left(Align::Center));
    let title = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).size(FONT_SIZE_TINY).family(semibold()).color(faint()));
    };
    cell(ui, rect, left + ROW_NAME_START, left + ROW_NAME_END, ltr, |ui| title(ui, t("App")));
    cell(ui, rect, left + ROW_VERSION_START, chip_x - ROW_VERSION_GAP, ltr, |ui| title(ui, t("Versión")));
    cell(ui, rect, chip_x, chip_x + ROW_CHIP_WIDTH, ltr, |ui| title(ui, t("Origen")));
    cell(ui, rect, size_x - 20.0, size_x + ROW_SIZE_WIDTH, rtl, |ui| title(ui, t("Instalada")));
}

/// One 68px row. Returns (its own "Actualizar"/"Reintentar" was clicked, the row body was clicked).
fn row(ui: &mut Ui, app: &App, status: Status, error: Option<&str>, selected: &mut bool, enabled: bool, icon: Option<&egui::TextureHandle>) -> (bool, bool) {
    // Per-app id scope: cells of different rows must not share widget ids.
    ui.push_id(&app.id, |ui| row_inner(ui, app, status, error, selected, enabled, icon)).inner
}

fn row_inner(ui: &mut Ui, app: &App, status: Status, error: Option<&str>, selected: &mut bool, enabled: bool, icon: Option<&egui::TextureHandle>) -> (bool, bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), ROW_HEIGHT), Sense::hover());
    // Not `resp.hovered()`: that goes false while the pointer is over the row's own buttons.
    if ui.rect_contains_pointer(rect) {
        ui.painter().rect_filled(rect, 0, tint(6));
    }
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, tint(10)));
    let Cols { left, right, chip_x, size_x, action_x } = Cols::of(rect);
    let ltr = Layout::left_to_right(Align::Center);
    let rtl = Layout::right_to_left(Align::Center);
    let mut clicked = false;
    // Registered before the cells so the checkbox and buttons drawn over it win their own clicks.
    let body = Rect::from_min_max(Pos2::new(left + ROW_ICON_START, rect.top()), Pos2::new(chip_x + ROW_CHIP_WIDTH, rect.bottom()));
    let shown = ui.interact(body, ui.make_persistent_id("open"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();

    cell(ui, rect, left, left + ROW_CHECKBOX_END, ltr, |ui| {
        ui.add_enabled(status == Status::Available, Checkbox::new(selected, ""));
    });
    cell(ui, rect, left + ROW_ICON_START, left + ROW_ICON_END, ltr, |ui| widgets::tile(ui, app, icon));
    cell(ui, rect, left + ROW_NAME_START, left + ROW_NAME_END, Layout::top_down(Align::Min), |ui| {
        ui.add_space(15.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add(egui::Label::new(RichText::new(&app.name).size(14.5).family(semibold())).truncate());
        // A failed row says why right under the name (the full text is on the button's tooltip).
        match (status, error) {
            (Status::Failed, Some(e)) => {
                let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(e);
                ui.add(egui::Label::new(RichText::new(tf1("Falló: {0}", first)).size(FONT_SIZE_MICRO).color(Color32::from_rgb(0xFF, 0x7A, 0x6B))).truncate()).on_hover_text(e);
            }
            _ => {
                ui.add(egui::Label::new(RichText::new(&app.vendor).size(FONT_SIZE_MICRO).color(faint())).truncate());
            }
        }
    });

    let (vx0, vx1) = (left + ROW_VERSION_START, chip_x - ROW_VERSION_GAP);
    // Two versions and an arrow share this cell: 8px gaps, 14px arrow, ~8px per mono char.
    let max_chars = (((vx1 - vx0 - 30.0) / 2.0) / 8.0).max(6.0) as usize;
    cell(ui, rect, vx0, vx1, ltr, |ui| {
        ui.spacing_mut().item_spacing.x = SPACING_SM;
        ui.label(RichText::new(widgets::short_version(&app.from, max_chars)).monospace().size(FONT_SIZE_SMALL).color(faint())).on_hover_text(&app.from);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
        widgets::icon(ui, r, Icon::Arrow, faint(), 2.2);
        ui.label(RichText::new(widgets::short_version(&app.to, max_chars)).monospace().size(FONT_SIZE_SMALL).color(accent_text())).on_hover_text(&app.to);
    });
    cell(ui, rect, chip_x, chip_x + ROW_CHIP_WIDTH, ltr, |ui| widgets::chip(ui, app.source.label()));
    cell(ui, rect, size_x, size_x + ROW_SIZE_WIDTH, rtl, |ui| {
        // Download size when known, else how long ago this version was installed.
        match (app.size_mb, app.installed_at) {
            (None, Some(t)) => {
                let short = model::ago_short(t, SystemTime::now());
                ui.label(RichText::new(short).monospace().size(FONT_SIZE_MICRO).color(faint())).on_hover_text(tf1("Instalada el {0}", &model::date(t)));
            }
            _ => {
                ui.label(RichText::new(model::format_size(app.size_mb)).monospace().size(FONT_SIZE_MICRO).color(faint()));
            }
        }
    });
    cell(ui, rect, action_x, right, rtl, |ui| match status {
        Status::Available => clicked = widgets::button(ui, None, t("Actualizar"), Kind::Soft, BUTTON_HEIGHT_SM, enabled).clicked(),
        Status::Failed => {
            let tip = error.unwrap_or(t("Error desconocido")).to_string();
            if app.source == Source::AppStore {
                // `mas` can fail where the App Store itself still works: offer to finish there.
                ui.vertical(|ui| {
                    ui.set_width(ROW_ACTION_WIDTH);
                    ui.add_space(8.0);
                    ui.spacing_mut().item_spacing.y = 3.0;
                    clicked = widgets::button(ui, None, t("Reintentar"), Kind::Soft, BUTTON_HEIGHT_SM, enabled).on_hover_text(tip).clicked();
                    let link = ui.add(egui::Label::new(RichText::new(t("Abrir App Store")).size(FONT_SIZE_MICRO).color(accent_text()).underline()).sense(Sense::click()));
                    if link.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                        let _ = std::process::Command::new("open").args(["-a", "App Store"]).spawn();
                    }
                });
            } else {
                clicked = widgets::button(ui, None, t("Reintentar"), Kind::Soft, BUTTON_HEIGHT_SM, enabled).on_hover_text(tip).clicked();
            }
        }
        Status::Queued => {
            ui.label(RichText::new(t("En cola")).size(FONT_SIZE_SMALL).color(faint()));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            widgets::icon(ui, r, Icon::Clock, faint(), 2.2);
        }
        Status::Updating(p) => {
            ui.vertical(|ui| {
                ui.set_width(ROW_ACTION_WIDTH);
                // Content is ~26px tall in a 68px row: push it to the vertical center.
                ui.add_space(21.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    let phase = if p < 70.0 { t("Descargando") } else { t("Instalando") };
                    ui.label(RichText::new(phase).size(FONT_SIZE_TINY).color(text_2()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}%", p.round())).monospace().size(FONT_SIZE_TINY).color(accent_text()));
                    });
                });
                widgets::progress_bar(ui, p / 100.0, 5.0);
            });
        }
        Status::Done => {
            ui.label(RichText::new(t("Actualizada")).size(FONT_SIZE_SMALL).family(semibold()).color(ok()));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            widgets::icon(ui, r, Icon::Check, ok(), 2.6);
        }
    });
    (clicked, shown)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn idle() -> Counts<'static> {
        Counts { scanning: false, error: None, warning: None, available: 0, total_mb: 0, busy: false, failed: 0 }
    }

    #[test]
    fn summary_picks_the_most_important_state() {
        assert_eq!(summary(&idle()), "Todo al día");
        assert_eq!(summary(&Counts { scanning: true, available: 3, ..idle() }), "Buscando actualizaciones…");
        assert_eq!(summary(&Counts { error: Some(Fail::Offline.text()), ..idle() }), "Sin conexión a internet");
        assert_eq!(summary(&Counts { error: Some("brew: boom"), ..idle() }), "No se pudo buscar: brew: boom");
        assert_eq!(summary(&Counts { available: 2, failed: 1, ..idle() }), "2 disponibles");
        assert_eq!(summary(&Counts { warning: Some("snap: x"), ..idle() }), "Revisión incompleta · snap: x");
    }
}
