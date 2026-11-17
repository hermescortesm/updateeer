#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

#[cfg(target_os = "macos")]
mod askpass;
mod autostart;
mod fail;
mod friendly;
mod history;
mod i18n;
mod managers;
mod model;
mod native;
mod notify;
mod permissions;
mod queue;
mod running;
mod selfupdate;
mod settings;
mod setup;
mod skip;
mod storage;
mod sudobridge;
mod tray;
mod ui;

// Short paths (`crate::scan`, `crate::theme`) for the modules used all over the app.
use managers::{catalog, discover, linuxpkg, local, mas, masroot, scan, upgrade, winpkg};
use ui::{icons, logos, pages, theme, widgets};

/// The Dock / window icon, drawn by `tools/gen_icon.py`.
fn app_icon() -> eframe::egui::IconData {
    let icon = icons::decode_bytes(include_bytes!("../assets/icon.png")).expect("assets/icon.png is a valid PNG");
    eframe::egui::IconData { rgba: icon.rgba, width: icon.size[0] as u32, height: icon.size[1] as u32 }
}

/// One instance only: a second launch (e.g. after "open at login") just exits.
fn single_instance() -> Option<std::fs::File> {
    let dir = settings::config_dir()?;
    std::fs::create_dir_all(&dir).ok()?;
    let file = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join("lock")).ok()?;
    match file.try_lock() {
        Err(std::fs::TryLockError::WouldBlock) => std::process::exit(0),
        _ => Some(file),
    }
}

/// `updateeer --root-shell`: what pkexec runs under the polkit policy. It is only a shell, so the
/// app can send administrator commands to it, but the policy names this exact program.
#[cfg(unix)]
fn root_shell() -> ! {
    use std::os::unix::process::CommandExt;
    let err = std::process::Command::new("/bin/sh").exec();
    eprintln!("updateeer: cannot start /bin/sh: {err}");
    std::process::exit(1)
}

fn main() -> eframe::Result {
    #[cfg(unix)]
    if std::env::args().nth(1).as_deref() == Some("--root-shell") {
        root_shell();
    }
    let _lock = single_instance();
    // Started by "open at login": stay in the menu bar until the user opens the window.
    let background = std::env::args().any(|a| a == "--background");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_visible(!background)
            .with_title("Updateeer")
            .with_app_id("updateeer")
            .with_icon(app_icon())
            .with_inner_size([theme::WINDOW_WIDTH, theme::WINDOW_HEIGHT])
            .with_min_inner_size([900.0, theme::WINDOW_MIN_HEIGHT]),
        ..Default::default()
    };
    eframe::run_native(
        "Updateeer",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx, settings::Settings::load().lang.unwrap_or_else(i18n::detect));
            Ok(Box::new(ui::UpdatesApp::new(cc.egui_ctx.clone(), background)))
        }),
    )
}
