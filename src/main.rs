#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod autostart;
mod catalog;
mod history;
mod discover;
mod icons;
mod linuxpkg;
mod local;
mod logos;
mod mas;
mod model;
mod native;
mod notify;
mod pages;
mod permissions;
mod queue;
mod running;
mod scan;
mod settings;
mod setup;
mod theme;
mod tray;
mod ui;
mod upgrade;
mod winpkg;
mod widgets;

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

fn main() -> eframe::Result {
    let _lock = single_instance();
    // Started by "open at login": stay in the menu bar until the user opens the window.
    let background = std::env::args().any(|a| a == "--background");
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_visible(!background)
            .with_title("Updateeer")
            .with_app_id("updateeer")
            .with_icon(app_icon())
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([1180.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Updateeer",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(ui::UpdatesApp::new(cc.egui_ctx.clone(), background)))
        }),
    )
}
