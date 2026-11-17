//! The main window: app state, the sidebar and the frame around each page.
mod banners;
mod containers;
mod disk;
pub mod icons;
pub mod logos;
pub mod pages;
pub mod theme;
mod updates;
pub mod widgets;

use std::collections::{HashMap, HashSet};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};
use std::process::Command;
use std::thread;

use eframe::egui::{
    self, Align, Checkbox, Color32, CornerRadius, FontId, Frame, Layout, Margin, Pos2, Rect, RichText,
    ScrollArea, Sense, Stroke, TextEdit, Ui, UiBuilder, Vec2,
};

use crate::fail::Fail;
use crate::i18n::{t, tf, tf1};
use crate::model::{self, App, Source};
use crate::queue::{Queue, Status};
use crate::history::{self, Entry};
use crate::native;
use crate::notify::{Notifier, Reply};
use crate::running::{self, State};
use crate::setup;
use crate::tray::{self, Tray};
use crate::autostart;
use crate::settings::{Settings, ThemeMode};
use crate::skip;
use crate::{mas, scan, upgrade};
use theme::*;
use widgets::{Icon, Kind};

// — Grouped state structs —

/// Background scan channel and its result.
pub(crate) struct ScanState {
    pub(crate) rx: Option<Receiver<Result<scan::Found, String>>>,
    pub(crate) error: Option<String>,
    pub(crate) warning: Option<String>,
    pub(crate) last_scan: Instant,
}

/// Updateeer self-update state.
pub(crate) struct SelfUpdateState {
    pub(crate) new_release: Option<crate::selfupdate::Release>,
    pub(crate) release_rx: Option<Receiver<Result<crate::selfupdate::Release, String>>>,
    pub(crate) self_update_rx: Option<Receiver<Result<(), String>>>,
    pub(crate) self_update_error: Option<String>,
    /// The progress bar as drawn: it eases toward the real progress.
    pub(crate) update_shown: f32,
    pub(crate) show_notes: bool,
}

/// Filter controls for the updates list.
pub(crate) struct FilterState {
    pub(crate) query: String,
    pub(crate) filter: Filter,
    pub(crate) kind: Type,
    pub(crate) age: Age,
    pub(crate) sort: Sort,
}

/// First-run setup state (macOS).
pub(crate) struct SetupState {
    pub(crate) tools: (bool, bool), // (Homebrew found, mas found)
    pub(crate) access: crate::permissions::Access,
    pub(crate) mas_install: Option<Receiver<Result<(), String>>>,
    pub(crate) setup_error: Option<String>,
    pub(crate) perm_prompted: bool,
}

/// Tray, quit, and background-launch state.
pub(crate) struct TrayState {
    pub(crate) tray: Option<Tray>,
    pub(crate) quitting: bool,
    /// Frames left before hiding the window (started with `--background`).
    pub(crate) hide_in: Option<u8>,
    pub(crate) login: bool,
    pub(crate) login_error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    All,
    Only(Source),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Updates,
    Installed,
    /// Catalog and search to install new apps.
    Discover,
    /// Disk space: what to clean and what takes the most room.
    Storage,
    History,
    Settings,
    /// First run: choose the interface language.
    Language,
    /// First-run checklist (macOS): permissions and tools.
    Welcome,
    /// One app's full data; `back` says where it was opened from.
    Detail,
}

type Scan = Receiver<Result<scan::Found, String>>;

/// Kind of thing: graphical apps vs command-line tools (Homebrew formulae).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Type {
    All,
    Apps,
    Tools,
}

/// How long ago the installed version arrived.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Age {
    Any,
    Week,
    Month,
    Old,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sort {
    Name,
    Newest,
    Oldest,
}

/// Source chips for this OS (the row is laid out right to left).
fn source_filters() -> Vec<(&'static str, Filter)> {
    if cfg!(windows) {
        vec![(t("Todas"), Filter::All), ("winget", Filter::Only(Source::Winget)), ("Scoop", Filter::Only(Source::Scoop)), ("Chocolatey", Filter::Only(Source::Chocolatey))]
    } else if cfg!(target_os = "linux") {
        // Only the managers this machine has, so the row stays short.
        let chips = [
            ("apt", "apt", Source::Apt),
            ("pacman", "pacman", Source::Pacman),
            ("aur", "AUR", Source::Aur),
            ("dnf", "dnf", Source::Dnf),
            ("zypper", "zypper", Source::Zypper),
            ("apk", "apk", Source::Apk),
            ("xbps", "xbps", Source::Xbps),
            ("flatpak", "Flatpak", Source::Flatpak),
            ("snap", "Snap", Source::Snap),
        ];
        let mut v = vec![(t("Todas"), Filter::All)];
        v.extend(chips.into_iter().filter(|(k, _, _)| crate::linuxpkg::path(k).is_some()).map(|(_, label, s)| (label, Filter::Only(s))));
        v.push((t("Directas"), Filter::Only(Source::Direct)));
        v
    } else {
        vec![(t("Todas"), Filter::All), ("App Store", Filter::Only(Source::AppStore)), ("Homebrew", Filter::Only(Source::Homebrew)), (t("Directas"), Filter::Only(Source::Direct))]
    }
}

fn detect_tools() -> (bool, bool) {
    (scan::brew_path().is_some(), mas::path().is_some())
}

type IconMsg = (String, Option<icons::Icon>);

pub struct UpdatesApp {
    apps: Vec<App>,
    selected: HashSet<String>,
    pub(crate) icons: HashMap<String, egui::TextureHandle>,
    icon_asked: HashSet<String>,
    icon_tx: Sender<IconMsg>,
    icon_rx: Receiver<IconMsg>,
    pub(crate) queue: Queue,
    pub(crate) ctx: egui::Context,
    done_tx: Sender<(String, Result<(), String>)>,
    done_rx: Receiver<(String, Result<(), String>)>,
    /// Background scan channel and its result.
    pub(crate) scan: ScanState,
    /// Updateeer self-update state.
    pub(crate) self_update: SelfUpdateState,
    /// Filter controls for the updates list.
    pub(crate) filter_state: FilterState,
    /// First-run setup state (macOS).
    pub(crate) setup: SetupState,
    /// Tray, quit, and background-launch state.
    pub(crate) tray_state: TrayState,
    pub(crate) page: Page,
    pub(crate) settings: Settings,
    pub(crate) installed: Vec<App>,
    pub(crate) installed_rx: Option<Scan>,
    pub(crate) discover: crate::pages::Discover,
    pub(crate) storage: disk::Storage,
    pub(crate) installed_stale: bool,
    pub(crate) installed_error: Option<String>,
    /// A source failed while others worked: shown so missing apps are not a mystery.
    pub(crate) installed_warning: Option<String>,
    pub(crate) history: Vec<Entry>,
    notifier: Notifier,
    notify_rx: Receiver<Reply>,
    /// Apps we already asked about (or were told "later" for): no new question before this time.
    snooze: HashMap<String, Instant>,
    /// "Update now" answers that arrived while a scan was replacing the queue.
    update_after_scan: Vec<String>,
    last_auto: Instant,
    was_focused: bool,
    last_logic: Instant,
    pub(crate) detail: Option<App>,
    pub(crate) back: Page,
    /// Skipped updates (id -> skipped version) and the apps hidden because of them.
    pub(crate) skipped: HashMap<String, String>,
    pub(crate) skipped_apps: Vec<App>,
    /// Scheduled update: the day it last ran, and a search that must end with an update pass.
    sched_day: Option<u32>,
    sched_run: bool,
    last_sched: Instant,
    /// Whether the queue was busy in the previous logic tick — detects batch completion.
    prev_batch_busy: bool,
}

impl UpdatesApp {
    pub fn new(ctx: egui::Context, start_hidden: bool) -> Self {
        let (done_tx, done_rx) = mpsc::channel();
        let (icon_tx, icon_rx) = mpsc::channel();
        let (notifier, notify_rx) = Notifier::new(&ctx);
        let mut settings = Settings::load();
        crate::i18n::set(settings.lang.unwrap_or_else(crate::i18n::detect));
        let login = autostart::enabled();
        let tools = detect_tools();
        let access = crate::permissions::app_management();
        // Detect version change: after an upgrade macOS revokes TCC permissions, so re-open settings.
        let current_version = env!("CARGO_PKG_VERSION");
        let version_changed = settings.last_version.as_deref() != Some(current_version);
        if version_changed {
            settings.last_version = Some(current_version.into());
            settings.save();
        }
        let mut app = Self {
            apps: Vec::new(),
            selected: HashSet::new(),
            icons: HashMap::new(),
            icon_asked: HashSet::new(),
            icon_tx,
            icon_rx,
            queue: Queue::new([], 5),
            ctx,
            done_tx,
            done_rx,
            scan: ScanState {
                rx: None,
                error: None,
                warning: None,
                last_scan: Instant::now(),
            },
            self_update: SelfUpdateState {
                new_release: None,
                release_rx: None,
                self_update_rx: None,
                self_update_error: None,
                update_shown: 0.0,
                show_notes: false,
            },
            filter_state: FilterState {
                query: String::new(),
                filter: Filter::All,
                kind: Type::All,
                age: Age::Any,
                sort: Sort::Name,
            },
            setup: SetupState {
                tools,
                access,
                mas_install: None,
                setup_error: None,
                perm_prompted: !(version_changed && access == crate::permissions::Access::Denied),
            },
            tray_state: TrayState {
                tray: None,
                quitting: false,
                hide_in: start_hidden.then_some(3),
                login,
                login_error: None,
            },
            page: if settings.lang.is_none() { Page::Language } else if cfg!(target_os = "macos") && !settings.onboarded { Page::Welcome } else { Page::Updates },
            settings,
            installed: Vec::new(),
            installed_rx: None,
            discover: crate::pages::Discover::new(),
            storage: disk::Storage::new(),
            installed_stale: true,
            installed_error: None,
            installed_warning: None,
            history: history::load(),
            notifier,
            notify_rx,
            snooze: HashMap::new(),
            update_after_scan: Vec::new(),
            last_auto: Instant::now(),
            was_focused: true,
            last_logic: Instant::now(),
            detail: None,
            back: Page::Updates,
            skipped: skip::load(),
            skipped_apps: Vec::new(),
            sched_day: None,
            sched_run: false,
            last_sched: Instant::now(),
            prev_batch_busy: false,
        };
        app.tray_state.tray = Tray::new(&app.ctx, app.tray_state.login);
        app.start_scan();
        // Read ahead what is installed, so Discover knows it before showing anything to install.
        if !start_hidden {
            app.start_installed();
        }
        let past: Vec<App> = app.history.iter().map(|e| e.app.clone()).collect();
        app.request_icons(&past);
        app
    }

    pub(crate) fn scanning(&self) -> bool {
        self.scan.rx.is_some()
    }

    fn offline(&self) -> bool {
        self.scan.error.as_deref().is_some_and(|e| Fail::Offline.is(e))
    }

    /// Looks for a newer Updateeer (same rhythm as the app search: startup, the chosen frequency,
    /// and every press of "Buscar actualizaciones").
    fn check_release(&mut self) {
        if !self.settings.check_updates || self.self_update.release_rx.is_some() {
            return;
        }
        let (tx, ctx) = (mpsc::channel(), self.ctx.clone());
        let sender = tx.0;
        thread::spawn(move || {
            let _ = sender.send(crate::selfupdate::latest());
            ctx.request_repaint();
        });
        self.self_update.release_rx = Some(tx.1);
    }

    /// Downloads, verifies and installs the new package, then the new version takes over.
    pub(crate) fn start_self_update(&mut self) {
        let Some(release) = self.self_update.new_release.clone().filter(|_| self.self_update.self_update_rx.is_none()) else { return };
        self.self_update.self_update_error = None;
        self.self_update.update_shown = 0.0;
        let (tx, ctx) = (mpsc::channel(), self.ctx.clone());
        let sender = tx.0;
        thread::spawn(move || {
            let _ = sender.send(crate::selfupdate::install(&release));
            ctx.request_repaint();
        });
        self.self_update.self_update_rx = Some(tx.1);
    }

    /// Phase, percentage and bar of the running self-update. The bar eases toward its target so a
    /// fast download is still seen filling up.
    pub(crate) fn update_progress(&mut self, ui: &mut Ui) {
        let (phase, target) = crate::selfupdate::progress();
        let dt = ui.input(|i| i.stable_dt).min(0.1);
        let shown = self.self_update.update_shown + (target - self.self_update.update_shown) * (1.0 - (-dt * 5.0).exp());
        self.self_update.update_shown = shown;
        ui.horizontal(|ui| {
            ui.label(RichText::new(phase).size(FONT_SIZE_TINY).color(text_2()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new(format!("{}%", (shown * 100.0).round())).monospace().size(FONT_SIZE_TINY).color(accent_text()));
            });
        });
        widgets::progress_bar(ui, shown, 5.0);
    }

    fn poll_self_update(&mut self) {
        let Some(rx) = &self.self_update.self_update_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.self_update.self_update_rx = None;
        match result {
            // The new version is already running; this old one just leaves.
            Ok(()) => std::process::exit(0),
            Err(e) => self.self_update.self_update_error = Some(e),
        }
    }

    fn poll_release(&mut self) {
        let Some(rx) = &self.self_update.release_rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.self_update.release_rx = None;
        // A failed check (offline, rate limit) is silent: nothing to tell the user.
        self.self_update.new_release = result.ok().filter(|r| crate::selfupdate::is_newer(env!("CARGO_PKG_VERSION"), &r.version) && crate::selfupdate::has_installer(r));
    }

    fn start_scan(&mut self) {
        self.check_release();
        if self.scanning() || self.queue.is_busy() {
            return;
        }
        self.setup.tools = detect_tools();
        self.scan.last_scan = Instant::now();
        let (tx, rx) = mpsc::channel();
        let ctx = self.ctx.clone();
        thread::spawn(move || {
            let _ = tx.send(scan::scan());
            ctx.request_repaint();
        });
        self.scan.rx = Some(rx);
        self.scan.error = None;
        self.scan.warning = None;
    }

    /// Auto-update: apps that are not running update on their own, open ones get a notification
    /// asking for "now" or "later". Off unless the user turned it on.
    fn auto_pass(&mut self, force: bool) {
        self.last_auto = Instant::now();
        if !(force || self.settings.auto_update) || self.scanning() {
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

    /// Scheduled update: once a day, at the chosen hour, search and then run the same pass as
    /// auto-update (idle apps update, open ones ask first).
    fn schedule_tick(&mut self) {
        let Some(hour) = self.settings.schedule else { return };
        if self.last_sched.elapsed() < Duration::from_secs(30) || self.scanning() || self.queue.is_busy() {
            return;
        }
        self.last_sched = Instant::now();
        let Some((day, now)) = local_day_hour() else { return };
        if now == u32::from(hour) && self.sched_day != Some(day) {
            self.sched_day = Some(day);
            self.sched_run = true;
            self.start_scan();
            if !self.scanning() {
                self.sched_run = false;
            }
        }
    }

    /// Hides this update until a newer version than `app.to` appears.
    pub(crate) fn skip_update(&mut self, app: &App) {
        if !matches!(self.queue.status(&app.id), Status::Available | Status::Failed) {
            return;
        }
        self.skipped.insert(app.id.clone(), app.to.clone());
        skip::save(&self.skipped);
        self.queue.forget(&app.id);
        self.selected.remove(&app.id);
        if let Some(i) = self.apps.iter().position(|a| a.id == app.id) {
            self.skipped_apps.push(self.apps.remove(i));
        }
    }

    pub(crate) fn unskip_update(&mut self, id: &str) {
        self.skipped.remove(id);
        skip::save(&self.skipped);
        if let Some(i) = self.skipped_apps.iter().position(|a| a.id == id) {
            let app = self.skipped_apps.remove(i);
            self.queue.track(&app.id);
            self.apps.push(app);
            self.apps.sort_by_key(|a| a.name.to_lowercase());
        }
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

    /// Show a system notification summarizing the batch that just finished.
    fn notify_batch_summary(&mut self, done: usize, failed: usize) {
        let title = if failed > 0 {
            tf("{0} actualizadas, {1} fallidas", &[done.to_string(), failed.to_string()])
        } else {
            tf1("{0} actualizadas", &done.to_string())
        };
        self.notifier.notify(&title, t("Las actualizaciones han terminado"));
    }

    fn poll_scan(&mut self) {
        let Some(rx) = &self.scan.rx else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.scan.rx = None;
        match result {
            Ok((apps, warning)) => {
                self.scan.warning = warning;
                let (skipped, apps): (Vec<App>, Vec<App>) = apps.into_iter().partition(|a| self.skipped.get(&a.id) == Some(&a.to));
                self.skipped_apps = skipped;
                self.request_icons(&apps);
                self.queue = Queue::new(apps.iter().map(|a| a.id.clone()), self.settings.parallel as usize);
                self.queue.set_sizes(apps.iter().filter_map(|a| Some((a.id.clone(), a.size_mb?))));
                self.apps = apps;
                self.selected.clear();
                let pending = std::mem::take(&mut self.update_after_scan);
                self.queue.enqueue(pending.iter().map(String::as_str));
                let scheduled = std::mem::take(&mut self.sched_run);
                self.auto_pass(scheduled);
            }
            Err(e) => self.scan.error = Some(e),
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
}

impl eframe::App for UpdatesApp {
    /// Runs on every wake-up, even while the window is covered or hidden (`ui` is skipped then):
    /// background results are collected and the queue keeps moving.
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Tray actions (open, scan, quit) must work while the window is hidden, when `ui` does not run.
        self.menu_bar();
        self.poll_mas_install();
        self.poll_notify();
        self.poll_scan();
        self.poll_release();
        self.poll_self_update();
        self.poll_icons();
        if self.self_update.self_update_rx.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        while let Ok((id, result)) = self.done_rx.try_recv() {
            let error = result.as_ref().err().cloned();
            if error.as_deref().is_some_and(|e| Fail::PasswordCancelled.is(e)) {
                self.queue.no_retry(&id);
            }
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
            self.auto_pass(false);
        }
        self.schedule_tick();
        if self.settings.auto || self.settings.auto_update || self.settings.schedule.is_some() {
            if self.settings.auto && self.scan.last_scan.elapsed() >= Duration::from_secs(self.settings.hours as u64 * 3600) {
                self.start_scan();
            }
            ctx.request_repaint_after(Duration::from_secs(60));
        }
        if self.queue.is_busy() {
            let dt = self.last_logic.elapsed().as_secs_f32();
            self.queue.creep(dt);
            if let Some(p) = crate::linuxpkg::pacman_progress() {
                self.queue.report("pacman:", p);
            }
            for id in self.queue.start_ready() {
                let (tx, ctx) = (self.done_tx.clone(), self.ctx.clone());
                thread::spawn(move || {
                    let result = upgrade::run(&id).map_err(scan::offline_or);
                    let _ = tx.send((id, result));
                    ctx.request_repaint();
                });
            }
            ctx.request_repaint_after(Duration::from_millis(100));
        }
        // Detect batch completion: queue was busy, now idle.
        let busy = self.queue.is_busy();
        if self.prev_batch_busy && !busy && self.queue.batch_summary().2 > 0 {
            let (done, failed, _total) = self.queue.batch_summary();
            self.notify_batch_summary(done, failed);
        }
        self.prev_batch_busy = busy;
        self.last_logic = Instant::now();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let light = match self.settings.theme {
            ThemeMode::Light => true,
            ThemeMode::Dark => false,
            ThemeMode::System => self.ctx.system_theme() == Some(egui::Theme::Light),
        };
        if light != theme::is_light() {
            theme::apply(&self.ctx, light);
        }
        // The user may have installed a tool in Terminal: look again when the window is focused.
        let focused = ui.input(|i| i.focused);
        if focused && !self.was_focused {
            self.setup.tools = detect_tools();
            self.setup.access = crate::permissions::app_management();
        }
        self.was_focused = focused;
        // Ask for «App Management» up front, not when the first update fails: open its pane once.
        if cfg!(target_os = "macos") && !self.setup.perm_prompted && self.setup.access == crate::permissions::Access::Denied && matches!(self.page, Page::Welcome | Page::Updates) {
            self.setup.perm_prompted = true;
            crate::permissions::open_settings();
        }
        // Responsive sidebar: collapses to icons-only when window is narrow.
        let narrow = ui.available_width() < 1000.0;
        let sidebar_w = if narrow { NARROW_SIDEBAR_WIDTH } else { SIDEBAR_WIDTH };
        egui::Panel::left("nav")
            .exact_size(sidebar_w)
            .resizable(false)
            .frame(Frame::new().fill(panel()).inner_margin(Margin::symmetric(14, 16)))
            .show(ui, |ui| self.sidebar(ui, narrow));
        egui::CentralPanel::default()
            .frame(Frame::new().fill(bg()).inner_margin(Margin { left: 40, right: 40, top: 40, bottom: 28 }))
            .show(ui, |ui| match self.page {
                Page::Updates => self.updates_page(ui),
                Page::Installed => self.installed_page(ui),
                Page::Discover => self.discover_page(ui),
                Page::Storage => self.storage_page(ui),
                Page::History => self.history_page(ui),
                // Long pages scroll as a whole; the list pages scroll their rows under a fixed header.
                Page::Settings => ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.settings_page(ui)).inner,
                Page::Language => ScrollArea::vertical().auto_shrink(false).show(ui, |ui| self.language_page(ui)).inner,
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
        if let Some(n) = self.tray_state.hide_in {
            self.tray_state.hide_in = n.checked_sub(1);
            if n == 0 {
                native::set_hidden(&ctx, true);
            } else {
                ctx.request_repaint();
            }
        }
        let Some(tray) = self.tray_state.tray.as_mut() else { return };
        if ctx.input(|i| i.viewport().close_requested()) && !self.tray_state.quitting {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            native::set_hidden(&ctx, true);
        }
        tray.update(self.queue.available_count(), self.scan.rx.is_some());
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
                tray::Action::Login => self.set_login(!self.tray_state.login),
                tray::Action::Quit => {
                    self.tray_state.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }
    }

    /// Turns "open at login" on or off, keeping the menu check and the setting in step.
    pub(crate) fn set_login(&mut self, on: bool) {
        match autostart::set(on) {
            Ok(()) => {
                self.tray_state.login = on;
                self.tray_state.login_error = None;
            }
            Err(e) => self.tray_state.login_error = Some(e),
        }
        if let Some(t) = &self.tray_state.tray {
            t.set_login(self.tray_state.login);
        }
    }

    fn sidebar(&mut self, ui: &mut Ui, narrow: bool) {
        ui.add_space(10.0);
        if !narrow {
            ui.horizontal(|ui| {
                ui.add_space(SPACING_SM);
                widgets::logo(ui, 26.0);
                ui.label(RichText::new("Updateeer").size(FONT_SIZE_TITLE).family(display()));
            });
            ui.add_space(SPACING_LG);
        }
        ui.spacing_mut().item_spacing.y = 2.0;
        let badge = Some(self.queue.available_count());
        for (page, icon, label) in [
            (Page::Updates, Icon::Download, t("Actualizaciones")),
            (Page::Installed, Icon::Grid, t("Instaladas")),
            (Page::Discover, Icon::Compass, t("Descubrir")),
            (Page::Storage, Icon::Disk, t("Almacenamiento")),
            (Page::History, Icon::Clock, t("Historial")),
            (Page::Settings, Icon::Sliders, t("Ajustes")),
        ] {
            let badge = if page == Page::Updates { badge } else { None };
            let current = if self.page == Page::Detail { self.back } else { self.page };
            if widgets::nav_item(ui, icon, label, current == page, badge).clicked() && self.settings.lang.is_some() {
                self.page = page;
            }
        }
        ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
            if !narrow {
                Frame::new()
                    .fill(raised())
                    .stroke(Stroke::new(1.0, tint(15)))
                    .corner_radius(12)
                    .inner_margin(14)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.spacing_mut().item_spacing.y = SPACING_XS;
                        let text = if self.settings.auto { tf1("Cada {0} h, con la app abierta", &self.settings.hours.to_string()) } else { t("Desactivada").into() };
                        ui.label(RichText::new(text).size(FONT_SIZE_TINY).color(faint()));
                        ui.label(RichText::new(t("Búsqueda automática")).size(FONT_SIZE_TINY).family(semibold()).color(text_2()));
                    });
                ui.add_space(10.0);
            }
            ui.horizontal(|ui| {
                ui.add_space(6.0);
                ui.label(RichText::new(tf1("Versión {0}", env!("CARGO_PKG_VERSION"))).size(FONT_SIZE_TINY).color(faint()));
            });
            if let Some(r) = &self.self_update.new_release {
                ui.add_space(6.0);
                let link = ui.horizontal(|ui| {
                    ui.add_space(6.0);
                    ui.add(egui::Label::new(RichText::new(tf1("Nueva versión {0} disponible", &r.version.clone())).size(FONT_SIZE_MICRO).family(semibold()).color(accent_text())).sense(Sense::click()))
                });
                let can_install = crate::selfupdate::can_self_install();
                let hint = if can_install { t("Ver en Ajustes") } else { t("Abrir la página de descarga") };
                if link.inner.on_hover_cursor(egui::CursorIcon::PointingHand).on_hover_text(hint).clicked() {
                    if can_install {
                        self.page = Page::Settings;
                    } else {
                        crate::selfupdate::open(&r.url);
                    }
                }
            }
        });
    }

    fn poll_mas_install(&mut self) {
        let Some(rx) = &self.setup.mas_install else { return };
        let Ok(result) = rx.try_recv() else { return };
        self.setup.mas_install = None;
        match result {
            Ok(()) => {
                self.setup.tools = detect_tools();
                self.start_scan();
            }
            Err(e) => self.setup.setup_error = Some(e),
        }
    }

    pub(crate) fn mas_install_running(&self) -> bool {
        self.setup.mas_install.is_some()
    }

    /// Opens the Homebrew installer, or installs `mas` once Homebrew is there.
    pub(crate) fn install_tool(&mut self, brew_installed: bool) {
        self.setup.setup_error = None;
        if !brew_installed {
            if let Err(e) = setup::open_homebrew_installer() {
                self.setup.setup_error = Some(e);
            }
        } else {
            let (tx, ctx) = (mpsc::channel(), self.ctx.clone());
            let sender = tx.0;
            thread::spawn(move || {
                let _ = sender.send(setup::install_mas());
                ctx.request_repaint();
            });
            self.setup.mas_install = Some(tx.1);
        }
    }
}

/// `(day number, hour)` on this computer's clock, from the system tools (no time library needed).
fn local_day_hour() -> Option<(u32, u32)> {
    let out = if cfg!(windows) {
        scan::run(Command::new("powershell"), &["-NoProfile", "-Command", "Get-Date -Format 'yyyyMMdd HH'"], Duration::from_secs(20)).ok()?
    } else {
        scan::run(Command::new("/bin/date"), &["+%Y%m%d %H"], Duration::from_secs(5)).ok()?
    };
    let (day, hour) = out.trim().split_once(' ')?;
    Some((day.parse().ok()?, hour.parse().ok()?))
}

/// Caption plus a dropdown over `options`.
pub(crate) fn combo<T: PartialEq + Copy>(ui: &mut Ui, caption: &str, value: &mut T, options: &[(T, &str)]) {
    ui.label(RichText::new(caption).size(FONT_SIZE_MICRO).color(faint()));
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
            .font(FontId::proportional(FONT_SIZE_BODY))
            .hint_text(t("Buscar app…"))
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
    let galley = ui.painter().layout_no_wrap(label.into(), FontId::new(FONT_SIZE_SMALL, semibold()), color);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(galley.size().x + 28.0, SEGMENT_HEIGHT), Sense::click());
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
