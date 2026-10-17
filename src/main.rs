mod history;
mod icons;
mod local;
mod logos;
mod mas;
mod model;
mod pages;
mod queue;
mod scan;
mod settings;
mod theme;
mod ui;
mod upgrade;
mod widgets;

/// The Dock / window icon, drawn by `tools/gen_icon.py`.
fn app_icon() -> eframe::egui::IconData {
    let icon = icons::decode_bytes(include_bytes!("../assets/icon.png")).expect("assets/icon.png is a valid PNG");
    eframe::egui::IconData { rgba: icon.rgba, width: icon.size[0] as u32, height: icon.size[1] as u32 }
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Updateeer")
            .with_icon(app_icon())
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([1180.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Updateeer",
        options,
        Box::new(|cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(ui::UpdatesApp::new(cc.egui_ctx.clone())))
        }),
    )
}
