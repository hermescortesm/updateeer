//! Window control that egui cannot do on its own. On macOS a hidden window stops getting frames, which would
//! freeze scans and the menu bar count, so "hidden" means fully transparent and click-through.
#[cfg(target_os = "macos")]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    /// Call on the main thread only (egui's `ui()` and tray callbacks both are).
    pub fn set_hidden(_ctx: &eframe::egui::Context, hidden: bool) {
        let Some(mtm) = MainThreadMarker::new() else { return };
        let app = NSApplication::sharedApplication(mtm);
        // No Dock icon while the app lives only in the menu bar.
        app.setActivationPolicy(if hidden { NSApplicationActivationPolicy::Accessory } else { NSApplicationActivationPolicy::Regular });
        for window in app.windows() {
            window.setAlphaValue(if hidden { 0.0 } else { 1.0 });
            window.setIgnoresMouseEvents(hidden);
        }
        if !hidden {
            #[allow(deprecated)]
            app.activateIgnoringOtherApps(true);
            for window in app.windows() {
                window.makeKeyAndOrderFront(None);
            }
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    /// Elsewhere a hidden window still gets frames, so "hidden" is simply minimized.
    pub fn set_hidden(ctx: &eframe::egui::Context, hidden: bool) {
        ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Minimized(hidden));
        if !hidden {
            ctx.send_viewport_cmd(eframe::egui::ViewportCommand::Focus);
        }
    }
}

pub use imp::set_hidden;
