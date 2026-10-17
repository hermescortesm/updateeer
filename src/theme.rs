use std::sync::Arc;

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};

pub const BG: Color32 = Color32::from_rgb(0x0D, 0x0E, 0x10);
pub const PANEL: Color32 = Color32::from_rgb(0x12, 0x13, 0x16);
pub const FIELD: Color32 = Color32::from_rgb(0x15, 0x17, 0x1A);
pub const BTN: Color32 = Color32::from_rgb(0x1C, 0x1F, 0x23);
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x2A);
pub const ACCENT_TEXT: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x70);
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x1A, 0x0F, 0x05);
pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF1, 0xEE);
pub const TEXT_2: Color32 = Color32::from_rgb(0xC9, 0xCB, 0xD0);
pub const MUTED: Color32 = Color32::from_rgb(0xA3, 0xA6, 0xAC);
pub const FAINT: Color32 = Color32::from_rgb(0x8B, 0x8E, 0x95);
pub const OK: Color32 = Color32::from_rgb(0x8F, 0xE0, 0xB6);
pub const LINE: Color32 = Color32::from_rgba_premultiplied(15, 15, 15, 15);

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Bricolage Grotesque, for the title and wordmark.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// Static, Latin-subset builds of the design's fonts (egui cannot use variable fonts).
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let embedded: [(&str, &[u8]); 4] = [
        ("geist", include_bytes!("../assets/fonts/Geist-Regular.ttf")),
        ("geist-semibold", include_bytes!("../assets/fonts/Geist-SemiBold.ttf")),
        ("geist-mono", include_bytes!("../assets/fonts/GeistMono-Regular.ttf")),
        ("bricolage", include_bytes!("../assets/fonts/Bricolage-Bold.ttf")),
    ];
    for (name, bytes) in embedded {
        fonts.font_data.insert(name.into(), Arc::new(FontData::from_static(bytes)));
    }
    set_families(&mut fonts, false);
    ctx.set_fonts(fonts.clone());

    // Fallback for scripts the subset lacks (CJK, accents in app names). These OS fonts are
    // tens of MB, so they load off the main thread and are added once ready.
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        let paths = ["/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/Helvetica.ttc"];
        let Some(bytes) = paths.into_iter().find_map(|p| std::fs::read(p).ok()) else { return };
        fonts.font_data.insert("os-fallback".into(), Arc::new(FontData::from_owned(bytes)));
        set_families(&mut fonts, true);
        ctx.set_fonts(fonts);
        ctx.request_repaint();
    });
}

fn set_families(fonts: &mut FontDefinitions, fallback: bool) {
    let chain = |own: &str| {
        let mut v = vec![own.to_string()];
        if fallback {
            v.push("os-fallback".into());
        }
        v
    };
    fonts.families.insert(FontFamily::Proportional, chain("geist"));
    fonts.families.insert(FontFamily::Monospace, chain("geist-mono"));
    fonts.families.insert(semibold(), chain("geist-semibold"));
    fonts.families.insert(display(), chain("bricolage"));
}

pub fn apply(ctx: &egui::Context) {
    install_fonts(ctx);
    let mut v = egui::Visuals::dark();
    v.panel_fill = BG;
    v.window_fill = PANEL;
    v.extreme_bg_color = FIELD;
    v.override_text_color = Some(TEXT);
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, ACCENT);
    let r = CornerRadius::same(10);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, LINE);
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, Color32::from_white_alpha(26));
    v.widgets.inactive.bg_fill = BTN;
    v.widgets.inactive.weak_bg_fill = BTN;
    v.widgets.hovered.weak_bg_fill = Color32::from_rgb(0x22, 0x25, 0x2A);
    v.widgets.hovered.bg_fill = Color32::from_rgb(0x22, 0x25, 0x2A);
    ctx.set_visuals(v);
}

/// Placeholder icon tile color: approximates the design's oklch(0.52 0.13 hue).
pub fn tile_color(hue: f32) -> Color32 {
    let h = hue.to_radians();
    // oklab from oklch, then to sRGB.
    let (l, c) = (0.52_f32, 0.13_f32);
    let (a, b) = (c * h.cos(), c * h.sin());
    let l_ = l + 0.396_337_78 * a + 0.215_803_76 * b;
    let m_ = l - 0.105_561_346 * a - 0.063_854_17 * b;
    let s_ = l - 0.089_484_18 * a - 1.291_485_5 * b;
    let (l3, m3, s3) = (l_.powi(3), m_.powi(3), s_.powi(3));
    let r = 4.076_741_7 * l3 - 3.307_711_6 * m3 + 0.230_969_93 * s3;
    let g = -1.268_438 * l3 + 2.609_757_4 * m3 - 0.341_319_4 * s3;
    let bl = -0.004_196_086 * l3 - 0.703_418_6 * m3 + 1.707_614_7 * s3;
    let enc = |x: f32| {
        let x = x.clamp(0.0, 1.0);
        let y = if x <= 0.003_130_8 { 12.92 * x } else { 1.055 * x.powf(1.0 / 2.4) - 0.055 };
        (y * 255.0).round() as u8
    };
    Color32::from_rgb(enc(r), enc(g), enc(bl))
}
