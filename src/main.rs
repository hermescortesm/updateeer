mod icons;
mod local;
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

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Updateeer")
            .with_inner_size([1200.0, 780.0])
            .with_min_inner_size([1180.0, 640.0]),
        ..Default::default()
    };
    eframe::run_native(
        "Updateeer",
        options,
        Box::new(|cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(ui::UpdatesApp::new(cc.egui_ctx.clone())))
        }),
    )
}
