use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};
use std::thread;

use eframe::egui::{
    self, Align, Checkbox, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder, Vec2,
};

use crate::model::{self, App, Source};
use crate::queue::{Queue, Status};
use crate::history::{self, Entry};
use crate::native;
use crate::notify::{Notifier, Reply};
use crate::running::{self, State};
use crate::setup;
use crate::tray::{self, Tray};
use crate::autostart;
use crate::icons;
use crate::settings::{Settings, ThemeMode};
use crate::{mas, scan, upgrade};
use crate::theme::{self, *};
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
    /// First-run checklist (macOS): permissions and tools.
    Welcome,
    /// One app's full data; `back` says where it was opened from.
    Detail,
}

type Scan = Receiver<Result<scan::Found, String>>;

/// Kind of thing: graphical apps vs command-line tools (Homebrew formulae).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Type {
    All,
    Apps,
    Tools,
}

/// How long ago the installed version arrived.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Age {
    Any,
    Week,
    Month,
    Old,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Sort {
    Name,
    Newest,
    Oldest,
}

/// Source chips for this OS (the row is laid out right to left).
fn source_filters() -> Vec<(&'static str, Filter)> {
    if cfg!(windows) {
        vec![("Todas", Filter::All), ("winget", Filter::Only(Source::Winget)), ("Scoop", Filter::Only(Source::Scoop)), ("Chocolatey", Filter::Only(Source::Chocolatey))]
    } else if cfg!(target_os = "linux") {
        vec![
            ("Todas", Filter::All),
            ("apt", Filter::Only(Source::Apt)),
            ("pacman", Filter::Only(Source::Pacman)),
            ("dnf", Filter::Only(Source::Dnf)),
            ("Flatpak", Filter::Only(Source::Flatpak)),
            ("Snap", Filter::Only(Source::Snap)),
        ]
    } else {
        vec![("Todas", Filter::All), ("App Store", Filter::Only(Source::AppStore)), ("Homebrew", Filter::Only(Source::Homebrew)), ("Directas", Filter::Only(Source::Direct))]
    }
}

fn detect_tools() -> (bool, bool) {
    (scan::brew_path().is_some(), mas::path().is_some())
}

type IconMsg = (String, Option<icons::Icon>);

pub struct UpdatesApp {
    apps: Vec<App>,
    selected: HashSet<String>,
    pub(crate) query: String,
    filter: Filter,
    kind: Type,
    age: Age,
    sort: Sort,
    pub(crate) icons: HashMap<String, egui::TextureHandle>,
    icon_asked: HashSet<String>,
    icon_tx: Sender<IconMsg>,
    icon_rx: Receiver<IconMsg>,
    pub(crate) queue: Queue,
    pub(crate) ctx: egui::Context,
    scan_rx: Option<Scan>,
    scan_error: Option<String>,
    scan_warning: Option<String>,
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
    tray: Option<Tray>,
    quitting: bool,
    notifier: Notifier,
    notify_rx: Receiver<Reply>,
    /// Apps we already asked about (or were told "later" for): no new question before this time.
    snooze: HashMap<String, Instant>,
    /// "Update now" answers that arrived while a scan was replacing the queue.
    update_after_scan: Vec<String>,
    last_auto: Instant,
    /// (Homebrew found, mas found).
    pub(crate) tools: (bool, bool),
    pub(crate) access: crate::permissions::Access,
    mas_install: Option<Receiver<Result<(), String>>>,
    pub(crate) setup_error: Option<String>,
    was_focused: bool,
    /// Frames left before hiding the window (started with `--background`).
    hide_in: Option<u8>,
    pub(crate) login: bool,
    pub(crate) login_error: Option<String>,
    pub(crate) detail: Option<App>,
    pub(crate) back: Page,
}

const CONCURRENCY: usize = 2;

impl UpdatesApp {
    pub fn new(ctx: egui::Context, start_hidden: bool) -> Self {
        let (done_tx, done_rx) = mpsc::channel();
        let (icon_tx, icon_rx) = mpsc::channel();
        let (notifier, notify_rx) = Notifier::new(&ctx);
        let settings = Settings::load();
        let mut app = Self {
            apps: Vec::new(),
            selected: HashSet::new(),
            query: String::new(),
            filter: Filter::All,
            kind: Type::All,
            age: Age::Any,
            sort: Sort::Name,
            icons: HashMap::new(),
            icon_asked: HashSet::new(),
            icon_tx,
            icon_rx,
            queue: Queue::new([], CONCURRENCY),
            ctx,
            scan_rx: None,
            scan_error: None,
            scan_warning: None,
            done_tx,
            done_rx,
            last_scan: Instant::now(),
            page: if cfg!(target_os = "macos") && !settings.onboarded { Page::Welcome } else { Page::Updates },
            settings,
            installed: Vec::new(),
            installed_rx: None,
            installed_stale: true,
            installed_error: None,
            history: history::load(),
            tray: None,
            quitting: false,
            notifier,
            notify_rx,
            snooze: HashMap::new(),
            update_after_scan: Vec::new(),
            last_auto: Instant::now(),
            tools: detect_tools(),
            access: crate::permissions::app_management(),
            mas_install: None,
            setup_error: None,
            was_focused: true,
            hide_in: start_hidden.then_some(3),
            login: autostart::enabled(),
            login_error: None,
            detail: None,
            back: Page::Updates,
        };
        app.tray = Tray::new(&app.ctx, app.login);
        app.start_scan();
        let past: Vec<App> = app.history.iter().map(|e| e.app.clone()).collect();
        app.request_icons(&past);
        app
    }

    pub(crate) fn scanning(&self) -> bool {
        self.scan_rx.is_some()
    }

    fn start_scan(&mut self) {
        if self.scanning() || self.queue.is_busy() {
            return;
        }
        self.tools = detect_tools();
        self.last_scan = Instant::now();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(scan::scan());
            ctx.request_repaint();
        });
        self.scan_rx = Some(rx);
        self.scan_error = None;
        self.scan_warning = None;
    }

    /// Auto-update: apps that are not running update on their own, open ones get a notification
    /// asking for "now" or "later". Off unless the user turned it on.
    fn auto_pass(&mut self) {
        self.last_auto = Instant::now();
        if !self.settings.auto_update || self.scanning() {
            return;
        }
        let procs = running::processes();
        let now = Instant::now();
        let mut idle = Vec::new();
        let mut ask = Vec::new();
        for app in self.apps.iter().filter(|a| self.queue.status(&a.id) == Status::Available) {
            match running::state(app, &procs) {
                State::Idle if !running::needs_admin(app) => idle.push(app.id.clone()),
                State::Idle | State::InUse | State::Unknown => {
                    if self.snooze.get(&app.id).is_none_or(|t| now >= *t) {
                        ask.push(app.clone());
                    }
                }
            }
        }
        // One question for all the open apps; ask again tomorrow at most, unless the answer says otherwise.
        for app in &ask {
            self.snooze.insert(app.id.clone(), now + Duration::from_secs(24 * 3600));
        }
        self.notifier.ask(&ask);
        self.queue.enqueue(idle.iter().map(String::as_str));
    }

    fn poll_notify(&mut self) {
        while let Ok(reply) = self.notify_rx.try_recv() {
            match reply {
                Reply::UpdateNow(ids) => {
                    for id in ids {
                        self.snooze.remove(&id);
                        if self.scanning() {
                            self.update_after_scan.push(id);
                        } else if matches!(self.queue.status(&id), Status::Available | Status::Failed) {
                            self.queue.enqueue([id.as_str()]);
                        }
                    }
                }
                Reply::Later(ids) => {
                    for id in ids {
                        self.snooze.insert(id, Instant::now() + Duration::from_secs(4 * 3600));
                    }
                }
                Reply::Open => native::set_hidden(&self.ctx, false),
            }
        }
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.scan_rx = None;
        match result {
            Ok((apps, warning)) => {
                self.scan_warning = warning;
                self.request_icons(&apps);
                self.queue = Queue::new(apps.iter().map(|a| a.id.clone()), CONCURRENCY);
                self.apps = apps;
                self.selected.clear();
                let pending = std::mem::take(&mut self.update_after_scan);
                self.queue.enqueue(pending.iter().map(String::as_str));
                self.auto_pass();
            }
            Err(e) => self.scan_error = Some(e),
        }
    }

    /// Loads real icons in the background, once per app; textures are made in `ui()`.
    pub(crate) fn request_icons(&mut self, apps: &[App]) {
        let todo: Vec<App> = apps.iter().filter(|a| icons::wants_icon(a) && self.icon_asked.insert(a.id.clone())).cloned().collect();
        if todo.is_empty() {
            return;
        }
        let (tx, ctx) = (self.icon_tx.clone(), self.ctx.clone());
        thread::spawn(move || {
            for app in todo {
                let _ = tx.send((app.id.clone(), icons::load(&app)));
                ctx.request_repaint();
            }
        });
    }

    fn poll_icons(&mut self) {
        while let Ok((id, icon)) = self.icon_rx.try_recv() {
            if let Some(icons::Icon { size, rgba }) = icon {
                let image = egui::ColorImage::from_rgba_unmultiplied(size, &rgba);
                let tex = self.ctx.load_texture(&id, image, egui::TextureOptions::LINEAR);
                self.icons.insert(id, tex);
            }
        }
    }

    /// Indices into `apps` that pass the search and filters, in the chosen order.
    fn visible(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        let now = SystemTime::now();
        let days = |a: &App| a.installed_at.map(|t| model::days_ago(t, now));
        let mut rows: Vec<usize> = (0..self.apps.len())
            .filter(|&i| {
                let a = &self.apps[i];
                let tool = a.id.starts_with("formula:") || matches!(a.source, Source::Apt | Source::Pacman | Source::Dnf);
                (match self.filter {
                    Filter::All => true,
                    Filter::Only(s) => a.source == s,
                }) && (match self.kind {
                    Type::All => true,
                    Type::Apps => !tool,
                    Type::Tools => tool,
                }) && (match self.age {
                    Age::Any => true,
                    Age::Week => days(a).is_some_and(|d| d <= 7),
                    Age::Month => days(a).is_some_and(|d| d <= 30),
                    Age::Old => days(a).is_some_and(|d| d > 90),
                }) && (q.is_empty() || a.name.to_lowercase().contains(&q))
            })
            .collect();
        // `apps` is already sorted by name; unknown dates always go last.
        match self.sort {
            Sort::Name => {}
            Sort::Newest => rows.sort_by_key(|&i| (self.apps[i].installed_at.is_none(), std::cmp::Reverse(self.apps[i].installed_at))),
            Sort::Oldest => rows.sort_by_key(|&i| (self.apps[i].installed_at.is_none(), self.apps[i].installed_at)),
        }
        rows
    }

    fn filters_active(&self) -> bool {
        (self.kind, self.age, self.sort) != (Type::All, Age::Any, Sort::Name)
    }
}

impl eframe::App for UpdatesApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let light = match self.settings.theme {
            ThemeMode::Light => true,
            ThemeMode::Dark => false,
            ThemeMode::System => self.ctx.system_theme() == Some(egui::Theme::Light),
        };
        if light != theme::is_light() {
            theme::apply(&self.ctx, light);
        }
        self.menu_bar();
        // The user may have installed a tool in Terminal: look again when the window is focused.
        let focused = ui.input(|i| i.focused);
        if focused && !self.was_focused {
            self.tools = detect_tools();
            self.access = crate::permissions::app_management();
        }
        self.was_focused = focused;
        self.poll_mas_install();
        self.poll_notify();
        self.poll_scan();
        self.poll_icons();
        while let Ok((id, result)) = self.done_rx.try_recv() {
            let error = result.as_ref().err().cloned();
            self.queue.finish(&id, result);
            // A failed first attempt is retried silently: only final outcomes make history.
            let app = self.apps.iter().find(|a| a.id == id);
            match (self.queue.status(&id), app) {
                (Status::Done, Some(a)) => {
                    self.history.push(Entry { app: a.clone(), error: None, at: SystemTime::now() });
                    history::save(&self.history);
                    self.installed_stale = true;
                }
                (Status::Failed, Some(a)) => {
                    self.history.push(Entry { app: a.clone(), error, at: SystemTime::now() });
                    history::save(&self.history);
                }
                _ => {}
            }
        }
        if self.settings.auto_update && self.last_auto.elapsed() >= Duration::from_secs(60) {
            self.auto_pass();
        }
        if self.settings.auto || self.settings.auto_update {
            if self.settings.auto && self.last_scan.elapsed() >= Duration::from_secs(self.settings.hours as u64 * 3600) {
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
            .frame(Frame::new().fill(panel()).inner_margin(Margin::symmetric(14, 16)))
            .show(ui, |ui| self.sidebar(ui));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(bg()).inner_margin(Margin { left: 40, right: 40, top: 40, bottom: 28 }))
            .show(ui, |ui| match self.page {
                Page::Updates => self.main(ui),
                Page::Installed => self.installed_page(ui),
                Page::History => self.history_page(ui),
                // Long pages scroll as a whole; the list pages scroll their rows under a fixed header.
                Page::Settings => ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.settings_page(ui)).inner,
                Page::Welcome => ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.welcome_page(ui)).inner,
                Page::Detail => ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.detail_page(ui)).inner,
            });
    }
}

impl UpdatesApp {
    /// Menu bar icon: closing the window hides it instead of quitting; the menu drives the rest.
    fn menu_bar(&mut self) {
        let ctx = self.ctx.clone();
        // eframe shows the window after its first frames; hide it once that is done.
        if let Some(n) = self.hide_in {
            self.hide_in = n.checked_sub(1);
            if n == 0 {
                native::set_hidden(&ctx, true);
            } else {
                ctx.request_repaint();
            }
        }
        let Some(tray) = self.tray.as_mut() else { return };
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            native::set_hidden(&ctx, true);
        }
        tray.update(self.queue.available_count(), self.scan_rx.is_some());
        let mut actions = Vec::new();
        while let Ok(a) = tray.rx.try_recv() {
            actions.push(a);
        }
        for action in actions {
            match action {
                tray::Action::Open => {
                    native::set_hidden(&ctx, false);
                }
                tray::Action::Scan => self.start_scan(),
                tray::Action::Login => self.set_login(!self.login),
                tray::Action::Quit => {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Turns "open at login" on or off, keeping the menu check and the setting in step.
    pub(crate) fn set_login(&mut self, on: bool) {
        match autostart::set(on) {
            Ok(()) => {
                self.login = on;
                self.login_error = None;
            }
            Err(e) => self.login_error = Some(e),
        }
        if let Some(t) = &self.tray {
            t.set_login(self.login);
        }
    }

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
            let current = if self.page == Page::Detail { self.back } else { self.page };
            if widgets::nav_item(ui, icon, label, current == page, badge).clicked() {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            Frame::new()
                .fill(raised())
                .stroke(Stroke::new(1.0, tint(15)))
                .corner_radius(12)
                .inner_margin(14)
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.spacing_mut().item_spacing.y = 4.0;
                    // The parent layout is bottom-up, so the lines are added in reverse.
                    let text = if self.settings.auto { format!("Cada {} h, con la app abierta", self.settings.hours) } else { "Desactivada".into() };
                    ui.label(RichText::new(text).size(12.0).color(faint()));
                    ui.label(RichText::new("Búsqueda automática").size(12.0).family(semibold()).color(text_2()));
                });
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(format!("Versión {}", env!("CARGO_PKG_VERSION"))).size(12.0).color(faint()));
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
        let mut summary = if self.scanning() {
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
        if let (Some(w), false) = (&self.scan_warning, self.scanning()) {
            summary = format!("{summary} · {w}");
        }
        // No actions while a scan runs: its result replaces the queue.
        let can_act = !self.scanning();

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.label(RichText::new("Actualizaciones").size(36.0).family(display()));
                ui.label(RichText::new(summary).size(14.0).color(muted()));
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
        if self.setup_banner(ui) {
            ui.add_space(22.0);
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
                                if segment(ui, label, self.filter == f).clicked() {
                                    self.filter = f;
                                }
                            }
                        });
                    });
                search_field(ui, &mut self.query);
            });
        });
        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;
            combo(ui, "Tipo", &mut self.kind, &[(Type::All, "Todo"), (Type::Apps, "Apps"), (Type::Tools, "Herramientas")]);
            ui.add_space(8.0);
            combo(ui, "Instalada", &mut self.age, &[(Age::Any, "Cualquiera"), (Age::Week, "Últimos 7 días"), (Age::Month, "Últimos 30 días"), (Age::Old, "Hace más de 3 meses")]);
            ui.add_space(8.0);
            combo(ui, "Orden", &mut self.sort, &[(Sort::Name, "Nombre"), (Sort::Newest, "Más recientes"), (Sort::Oldest, "Más antiguas")]);
            if self.filters_active() && widgets::button(ui, None, "Limpiar", Kind::Soft, 34.0, true).clicked() {
                (self.kind, self.age, self.sort) = (Type::All, Age::Any, Sort::Name);
            }
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
        let mut open: Option<App> = None;
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
                inner.label(RichText::new(label).size(13.0).family(semibold()).color(text_2()));
                inner.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if widgets::button(ui, None, "Actualizar seleccionadas", Kind::Primary, 34.0, n > 0 && can_act).clicked() {
                        to_update.extend(visible_ids.iter().filter(|id| self.selected.contains(*id)).cloned());
                    }
                });
                column_titles(ui);
                ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
                    if rows.is_empty() {
                        ui.add_space(64.0);
                        ui.vertical_centered(|ui| {
                            let (msg, color) = match (&self.scan_error, self.scanning(), self.apps.is_empty()) {
                                (Some(e), false, _) => (format!("No se pudo buscar: {e}"), muted()),
                                (_, true, _) => ("Buscando…".into(), faint()),
                                (_, _, true) => ("No hay actualizaciones.".into(), faint()),
                                _ => ("Ninguna app coincide con tu búsqueda.".into(), faint()),
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
        if !to_update.is_empty() {
            self.queue.enqueue(to_update.iter().map(String::as_str));
            for id in &to_update {
                self.selected.remove(id);
            }
        }
    }

    fn poll_mas_install(&mut self) {
        let Some(rx) = &self.mas_install else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.mas_install = None;
        match result {
            Ok(()) => {
                self.tools = detect_tools();
                self.start_scan();
            }
            Err(e) => self.setup_error = Some(e),
        }
    }

    /// Offers to install Homebrew or `mas` (macOS). Nothing is installed without a click.
    /// Returns whether it drew anything.
    fn setup_banner(&mut self, ui: &mut Ui) -> bool {
        let (brew, mas) = self.tools;
        if !cfg!(target_os = "macos") || self.settings.hide_setup || (brew && mas) {
            return false;
        }
        let installing = self.mas_install.is_some();
        let (title, text, action) = if !brew {
            (
                "Falta Homebrew",
                "Updateeer lo usa para buscar y actualizar tus apps. Se instala desde Terminal y te pedirá tu contraseña.",
                "Instalar Homebrew",
            )
        } else {
            (
                "Falta mas para las apps de la App Store",
                "Es una herramienta pequeña que se instala con Homebrew. Sin ella no se ven las actualizaciones de la App Store.",
                "Instalar mas",
            )
        };
        let mut install = false;
        let mut dismiss = false;
        Frame::new()
            .fill(banner_bg())
            .stroke(Stroke::new(1.0, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 77)))
            .corner_radius(14)
            .inner_margin(Margin::symmetric(18, 16))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 12.0;
                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width() - 300.0);
                        ui.spacing_mut().item_spacing.y = 4.0;
                        ui.label(RichText::new(title).size(14.0).family(semibold()));
                        let detail = self.setup_error.clone().unwrap_or_else(|| text.to_string());
                        ui.label(RichText::new(detail).size(13.0).color(muted()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let label = if installing { "Instalando…" } else { action };
                        install = widgets::button(ui, None, label, Kind::Primary, 36.0, !installing).clicked();
                        dismiss = widgets::button(ui, None, "Ahora no", Kind::Soft, 36.0, !installing).clicked();
                    });
                });
            });
        if dismiss {
            self.settings.hide_setup = true;
            self.settings.save();
        }
        if install {
            self.install_tool(brew);
        }
        true
    }

    /// Opens the Homebrew installer, or installs `mas` once Homebrew is there.
    pub(crate) fn mas_install_running(&self) -> bool {
        self.mas_install.is_some()
    }

    pub(crate) fn install_tool(&mut self, brew_installed: bool) {
        self.setup_error = None;
        if !brew_installed {
            if let Err(e) = setup::open_homebrew_installer() {
                self.setup_error = Some(e);
            }
        } else {
            let (tx, ctx) = (mpsc::channel(), self.ctx.clone());
            let sender = tx.0;
            thread::spawn(move || {
                let _ = sender.send(setup::install_mas());
                ctx.request_repaint();
            });
            self.mas_install = Some(tx.1);
        }
    }

    fn batch_banner(&mut self, ui: &mut Ui) {
        let (done, total, pct) = self.queue.batch_progress();
        Frame::new()
            .fill(banner_bg())
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
                                ui.label(RichText::new(format!("{}%", pct.round())).monospace().color(accent_text()));
                            });
                        });
                        widgets::progress_bar(ui, pct / 100.0, 6.0);
                    });
                    if widgets::button(ui, None, "Cancelar", Kind::Soft, 36.0, true).clicked() {
                        self.queue.cancel();
                        scan::cancel_running();
                    }
                });
            });
    }
}

/// Caption plus a dropdown over `options`.
fn combo<T: PartialEq + Copy>(ui: &mut Ui, caption: &str, value: &mut T, options: &[(T, &str)]) {
    ui.label(RichText::new(caption).size(12.5).color(faint()));
    let current = options.iter().find(|o| o.0 == *value).map_or("", |o| o.1);
    egui::ComboBox::from_id_salt(caption).selected_text(current).width(150.0).show_ui(ui, |ui| {
        for (v, label) in options {
            ui.selectable_value(value, *v, *label);
        }
    });
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
    widgets::icon(ui, Rect::from_center_size(c, Vec2::splat(16.0)), Icon::Search, faint(), 2.2);
}

pub(crate) fn segment(ui: &mut Ui, label: &str, on: bool) -> egui::Response {
    let color = if on { text() } else { faint() };
    let galley = ui.painter().layout_no_wrap(label.into(), FontId::new(13.0, semibold()), color);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 28.0, 34.0), Sense::click());
    if on {
        ui.painter().rect_filled(rect, CornerRadius::same(8), segment_on());
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
        let (left, right) = (rect.left() + 18.0, rect.right() - 18.0);
        let action_x = right - 150.0;
        let size_x = action_x - 14.0 - 64.0;
        let chip_x = size_x - 14.0 - 86.0;
        Self { left, right, chip_x, size_x, action_x }
    }
}

/// Titles above the columns of the updates table, aligned with `row_inner`.
fn column_titles(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 34.0), Sense::hover());
    ui.painter().hline(rect.x_range(), rect.bottom() - 0.5, Stroke::new(1.0, line()));
    let Cols { left, chip_x, size_x, .. } = Cols::of(rect);
    let (ltr, rtl) = (Layout::left_to_right(Align::Center), Layout::right_to_left(Align::Center));
    let title = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).size(12.0).family(semibold()).color(faint()));
    };
    cell(ui, rect, left + 86.0, left + 296.0, ltr, |ui| title(ui, "App"));
    cell(ui, rect, left + 310.0, chip_x - 14.0, ltr, |ui| title(ui, "Versión"));
    cell(ui, rect, chip_x, chip_x + 86.0, ltr, |ui| title(ui, "Origen"));
    cell(ui, rect, size_x - 20.0, size_x + 64.0, rtl, |ui| title(ui, "Instalada"));
}

/// One 68px row. Returns (its own "Actualizar"/"Reintentar" was clicked, the row body was clicked).
fn row(ui: &mut Ui, app: &App, status: Status, error: Option<&str>, selected: &mut bool, enabled: bool, icon: Option<&egui::TextureHandle>) -> (bool, bool) {
    // Per-app id scope: cells of different rows must not share widget ids.
    ui.push_id(&app.id, |ui| row_inner(ui, app, status, error, selected, enabled, icon)).inner
}

fn row_inner(ui: &mut Ui, app: &App, status: Status, error: Option<&str>, selected: &mut bool, enabled: bool, icon: Option<&egui::TextureHandle>) -> (bool, bool) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 68.0), Sense::hover());
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
    let body = Rect::from_min_max(Pos2::new(left + 32.0, rect.top()), Pos2::new(chip_x + 86.0, rect.bottom()));
    let shown = ui.interact(body, ui.make_persistent_id("open"), Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand).clicked();

    cell(ui, rect, left, left + 18.0, ltr, |ui| {
        ui.add_enabled(status == Status::Available, Checkbox::new(selected, ""));
    });
    cell(ui, rect, left + 32.0, left + 72.0, ltr, |ui| widgets::tile(ui, app, icon));
    cell(ui, rect, left + 86.0, left + 296.0, Layout::top_down(Align::Min), |ui| {
        ui.add_space(15.0);
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.add(egui::Label::new(RichText::new(&app.name).size(14.5).family(semibold())).truncate());
        // A failed row says why right under the name (the full text is on the button's tooltip).
        match (status, error) {
            (Status::Failed, Some(e)) => {
                let first = e.lines().find(|l| !l.trim().is_empty()).unwrap_or(e);
                ui.add(egui::Label::new(RichText::new(format!("Falló: {first}")).size(12.5).color(Color32::from_rgb(0xFF, 0x7A, 0x6B))).truncate()).on_hover_text(e);
            }
            _ => {
                ui.add(egui::Label::new(RichText::new(&app.vendor).size(12.5).color(faint())).truncate());
            }
        }
    });

    let (vx0, vx1) = (left + 310.0, chip_x - 14.0);
    // Two versions and an arrow share this cell: 8px gaps, 14px arrow, ~8px per mono char.
    let max_chars = (((vx1 - vx0 - 30.0) / 2.0) / 8.0).max(6.0) as usize;
    cell(ui, rect, vx0, vx1, ltr, |ui| {
        ui.spacing_mut().item_spacing.x = 8.0;
        ui.label(RichText::new(widgets::short_version(&app.from, max_chars)).monospace().size(13.0).color(faint())).on_hover_text(&app.from);
        let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
        widgets::icon(ui, r, Icon::Arrow, faint(), 2.2);
        ui.label(RichText::new(widgets::short_version(&app.to, max_chars)).monospace().size(13.0).color(accent_text())).on_hover_text(&app.to);
    });
    cell(ui, rect, chip_x, chip_x + 86.0, ltr, |ui| widgets::chip(ui, app.source.label()));
    cell(ui, rect, size_x, size_x + 64.0, rtl, |ui| {
        // Download size when known, else how long ago this version was installed.
        match (app.size_mb, app.installed_at) {
            (None, Some(t)) => {
                let short = model::ago(t, SystemTime::now()).replace("hace ", "");
                ui.label(RichText::new(short).monospace().size(12.5).color(faint())).on_hover_text(format!("Instalada el {}", model::date(t)));
            }
            _ => {
                ui.label(RichText::new(model::format_size(app.size_mb)).monospace().size(12.5).color(faint()));
            }
        }
    });
    cell(ui, rect, action_x, right, rtl, |ui| match status {
        Status::Available => clicked = widgets::button(ui, None, "Actualizar", Kind::Soft, 34.0, enabled).clicked(),
        Status::Failed => {
            let tip = error.unwrap_or("Error desconocido").to_string();
            clicked = widgets::button(ui, None, "Reintentar", Kind::Soft, 34.0, enabled).on_hover_text(tip).clicked();
        }
        Status::Queued => {
            ui.label(RichText::new("En cola").size(13.0).color(faint()));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(14.0), Sense::hover());
            widgets::icon(ui, r, Icon::Clock, faint(), 2.2);
        }
        Status::Updating(p) => {
            ui.vertical(|ui| {
                ui.set_width(150.0);
                // Content is ~26px tall in a 68px row: push it to the vertical center.
                ui.add_space(21.0);
                ui.spacing_mut().item_spacing.y = 6.0;
                ui.horizontal(|ui| {
                    let phase = if p < 70.0 { "Descargando" } else { "Instalando" };
                    ui.label(RichText::new(phase).size(12.0).color(text_2()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(RichText::new(format!("{}%", p.round())).monospace().size(12.0).color(accent_text()));
                    });
                });
                widgets::progress_bar(ui, p / 100.0, 5.0);
            });
        }
        Status::Done => {
            ui.label(RichText::new("Actualizada").size(13.0).family(semibold()).color(ok()));
            let (r, _) = ui.allocate_exact_size(Vec2::splat(16.0), Sense::hover());
            widgets::icon(ui, r, Icon::Check, ok(), 2.6);
        }
    });
    (clicked, shown)
}
