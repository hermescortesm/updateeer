//! Window control that egui cannot do. On macOS a hidden window stops getting frames, which would
//! freeze scans and the menu bar count, so "hidden" means fully transparent and click-through.
#[cfg(target_os = "macos")]
mod imp {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};

    /// Call on the main thread only (egui's `ui()` and tray callbacks both are).
    pub fn set_hidden(hidden: bool) {
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
    pub fn set_hidden(_hidden: bool) {}
}

pub use imp::set_hidden;
