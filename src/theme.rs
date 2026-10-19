use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use eframe::egui::{self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Stroke};

/// Same in both themes.
pub const ACCENT: Color32 = Color32::from_rgb(0xFF, 0x8A, 0x2A);
pub const ON_ACCENT: Color32 = Color32::from_rgb(0x1A, 0x0F, 0x05);

static LIGHT: AtomicBool = AtomicBool::new(false);

pub fn is_light() -> bool {
    LIGHT.load(Ordering::Relaxed)
}

/// Colors that change with the theme: `name: dark, light`.
macro_rules! palette {
    ($($name:ident: $dark:expr, $light:expr;)*) => {
        $(pub fn $name() -> Color32 { if is_light() { $light } else { $dark } })*
    };
}

palette! {
    bg: Color32::from_rgb(0x0D, 0x0E, 0x10), Color32::from_rgb(0xF5, 0xF4, 0xF1);
    panel: Color32::from_rgb(0x12, 0x13, 0x16), Color32::from_rgb(0xFF, 0xFF, 0xFF);
    field: Color32::from_rgb(0x15, 0x17, 0x1A), Color32::from_rgb(0xFF, 0xFF, 0xFF);
    btn: Color32::from_rgb(0x1C, 0x1F, 0x23), Color32::from_rgb(0xEE, 0xEC, 0xE7);
    raised: Color32::from_rgb(0x17, 0x19, 0x1C), Color32::from_rgb(0xFA, 0xF9, 0xF7);
    raised_hover: Color32::from_rgb(0x22, 0x25, 0x2A), Color32::from_rgb(0xE9, 0xE7, 0xE1);
    segment_on: Color32::from_rgb(0x2A, 0x2D, 0x33), Color32::from_rgb(0xE6, 0xE3, 0xDC);
    banner_bg: Color32::from_rgb(0x1A, 0x15, 0x10), Color32::from_rgb(0xFF, 0xF1, 0xE2);
    accent_text: Color32::from_rgb(0xFF, 0xB0, 0x70), Color32::from_rgb(0xB8, 0x4E, 0x00);
    text: Color32::from_rgb(0xF2, 0xF1, 0xEE), Color32::from_rgb(0x15, 0x16, 0x1A);
    text_2: Color32::from_rgb(0xC9, 0xCB, 0xD0), Color32::from_rgb(0x3A, 0x3D, 0x44);
    muted: Color32::from_rgb(0xA3, 0xA6, 0xAC), Color32::from_rgb(0x5A, 0x5E, 0x66);
    faint: Color32::from_rgb(0x8B, 0x8E, 0x95), Color32::from_rgb(0x74, 0x78, 0x80);
    ok: Color32::from_rgb(0x8F, 0xE0, 0xB6), Color32::from_rgb(0x1B, 0x8A, 0x57);
    line: Color32::from_white_alpha(15), Color32::from_black_alpha(22);
}

/// Overlay of the given strength: white on dark, black on light (hovers, borders, tracks).
pub fn tint(alpha: u8) -> Color32 {
    if is_light() { Color32::from_black_alpha(alpha) } else { Color32::from_white_alpha(alpha) }
}

pub fn semibold() -> FontFamily {
    FontFamily::Name("semibold".into())
}

/// Bricolage Grotesque, for the title and wordmark.
pub fn display() -> FontFamily {
    FontFamily::Name("display".into())
}

/// Static, Latin-subset builds of the design's fonts (egui cannot use variable fonts).
pub fn install_fonts(ctx: &egui::Context, lang: crate::i18n::Lang) {
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
        let Some(bytes) = fallback_font_paths(lang).into_iter().find_map(|p| std::fs::read(p).ok()) else { return };
        fonts.font_data.insert("os-fallback".into(), Arc::new(FontData::from_owned(bytes)));
        set_families(&mut fonts, true);
        ctx.set_fonts(fonts);
        ctx.request_repaint();
    });
}

/// OS fonts that cover Chinese and Japanese, best match for `lang` first. macOS always loads one
/// (accents in app names); Windows and Linux only when the interface needs CJK.
fn fallback_font_paths(lang: crate::i18n::Lang) -> Vec<&'static str> {
    use crate::i18n::Lang;
    let cjk = matches!(lang, Lang::Ja | Lang::Zh);
    let mut paths = Vec::new();
    if cfg!(target_os = "macos") {
        if lang == Lang::Ja {
            paths.push("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc");
        }
        if lang == Lang::Zh {
            paths.push("/System/Library/Fonts/PingFang.ttc");
        }
        paths.extend(["/System/Library/Fonts/Hiragino Sans GB.ttc", "/System/Library/Fonts/Helvetica.ttc"]);
    } else if cjk && cfg!(windows) {
        paths.extend(if lang == Lang::Ja { ["C:\\Windows\\Fonts\\YuGothR.ttc", "C:\\Windows\\Fonts\\meiryo.ttc", "C:\\Windows\\Fonts\\msyh.ttc"] } else { ["C:\\Windows\\Fonts\\msyh.ttc", "C:\\Windows\\Fonts\\simsun.ttc", "C:\\Windows\\Fonts\\YuGothR.ttc"] });
    } else if cjk {
        paths.extend([
            "/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/noto/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/OTF/NotoSansCJK-Regular.ttc",
            "/usr/share/fonts/adobe-source-han-sans/SourceHanSans-Regular.ttc",
            "/usr/share/fonts/truetype/wqy/wqy-microhei.ttc",
            "/usr/share/fonts/wenquanyi/wqy-microhei/wqy-microhei.ttc",
            "/usr/share/fonts/TTF/wqy-microhei.ttc",
        ]);
    }
    paths
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

/// Loads fonts and starts in the dark palette; the app switches it from the setting.
pub fn install(ctx: &egui::Context, lang: crate::i18n::Lang) {
    install_fonts(ctx, lang);
    apply(ctx, false);
}

/// Switches the whole palette and egui's visuals.
pub fn apply(ctx: &egui::Context, light: bool) {
    LIGHT.store(light, Ordering::Relaxed);
    let mut v = if light { egui::Visuals::light() } else { egui::Visuals::dark() };
    v.panel_fill = bg();
    v.window_fill = panel();
    v.extreme_bg_color = field();
    v.override_text_color = Some(text());
    // Selected text is drawn in `selection.stroke` color: keep it readable on the fill.
    if light {
        v.selection.bg_fill = Color32::from_rgb(0xFF, 0xCF, 0xA3);
        v.selection.stroke = Stroke::new(1.0, ON_ACCENT);
    } else {
        v.selection.bg_fill = Color32::from_rgb(0xB5, 0x55, 0x12);
        v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    }
    let r = CornerRadius::same(10);
    for w in [
        &mut v.widgets.noninteractive,
        &mut v.widgets.inactive,
        &mut v.widgets.hovered,
        &mut v.widgets.active,
    ] {
        w.corner_radius = r;
    }
    v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, line());
    v.widgets.inactive.bg_stroke = Stroke::new(1.0, tint(26));
    v.widgets.inactive.bg_fill = btn();
    v.widgets.inactive.weak_bg_fill = btn();
    v.widgets.hovered.weak_bg_fill = raised_hover();
    v.widgets.hovered.bg_fill = raised_hover();
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
