//! Painter-drawn pieces so the app looks the same on every OS.
use eframe::egui::{self, Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2};

use crate::model::App;
use crate::theme::*;

#[derive(Clone, Copy)]
pub enum Icon {
    Download,
    Grid,
    Clock,
    Sliders,
    Search,
    Refresh,
    Arrow,
    Check,
}

/// Draws a 24x24 stroke icon scaled to `rect`.
pub fn icon(ui: &Ui, rect: Rect, kind: Icon, color: Color32, width: f32) {
    let s = rect.width() / 24.0;
    let p = |x: f32, y: f32| rect.min + Vec2::new(x, y) * s;
    let stroke = Stroke::new(width * s, color);
    let painter = ui.painter();
    let line = |pts: &[(f32, f32)]| {
        for w in pts.windows(2) {
            painter.line_segment([p(w[0].0, w[0].1), p(w[1].0, w[1].1)], stroke);
        }
    };
    match kind {
        Icon::Download => {
            line(&[(12.0, 3.0), (12.0, 15.0)]);
            line(&[(7.0, 10.0), (12.0, 15.0), (17.0, 10.0)]);
            line(&[(5.0, 20.0), (19.0, 20.0)]);
        }
        Icon::Grid => {
            for (x, y) in [(3.5, 3.5), (13.5, 3.5), (3.5, 13.5), (13.5, 13.5)] {
                painter.rect_stroke(Rect::from_min_size(p(x, y), Vec2::splat(7.0 * s)), CornerRadius::same((2.0 * s) as u8), stroke, egui::StrokeKind::Inside);
            }
        }
        Icon::Clock => {
            painter.circle_stroke(p(12.0, 12.0), 8.5 * s, stroke);
            line(&[(12.0, 7.5), (12.0, 12.0), (15.0, 14.0)]);
        }
        Icon::Sliders => {
            line(&[(4.0, 7.0), (14.0, 7.0)]);
            line(&[(18.0, 7.0), (20.0, 7.0)]);
            line(&[(4.0, 17.0), (8.0, 17.0)]);
            line(&[(12.0, 17.0), (20.0, 17.0)]);
            painter.circle_stroke(p(16.0, 7.0), 2.0 * s, stroke);
            painter.circle_stroke(p(10.0, 17.0), 2.0 * s, stroke);
        }
        Icon::Search => {
            painter.circle_stroke(p(11.0, 11.0), 6.5 * s, stroke);
            line(&[(20.0, 20.0), (15.8, 15.8)]);
        }
        Icon::Refresh => {
            // Arc from 0° round to 315° (clockwise on screen) plus the arrow head.
            let arc: Vec<(f32, f32)> = (0..=24)
                .map(|i| {
                    let a = (i as f32 / 24.0 * 315.0).to_radians();
                    (12.0 + 8.0 * a.cos(), 12.0 + 8.0 * a.sin())
                })
                .collect();
            line(&arc);
            line(&[(20.0, 4.0), (20.0, 9.0), (15.0, 9.0)]);
        }
        Icon::Arrow => {
            line(&[(5.0, 12.0), (19.0, 12.0)]);
            line(&[(13.0, 6.0), (19.0, 12.0), (13.0, 18.0)]);
        }
        Icon::Check => line(&[(5.0, 12.5), (9.5, 17.0), (19.0, 7.5)]),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Primary,
    Ghost,
    Soft,
}

/// Button as drawn in the design: optional icon, semibold label, 1px border on non-primary.
pub fn button(ui: &mut Ui, ic: Option<Icon>, label: &str, kind: Kind, h: f32, enabled: bool) -> Response {
    let (size, pad, radius) = if h >= 44.0 { (14.0, 18.0, 11) } else { (13.0, 15.0, 9) };
    let (fill, hover, border, fg) = match kind {
        Kind::Primary => (ACCENT, Color32::from_rgb(0xFF, 0x9C, 0x48), Color32::TRANSPARENT, ON_ACCENT),
        Kind::Ghost => (Color32::from_rgb(0x17, 0x19, 0x1C), Color32::from_rgb(0x22, 0x25, 0x2A), Color32::from_white_alpha(31), TEXT),
        Kind::Soft => (BTN, Color32::from_rgb(0x22, 0x25, 0x2A), Color32::from_white_alpha(31), TEXT),
    };
    let galley = ui.painter().layout_no_wrap(label.into(), FontId::new(size, semibold()), fg);
    let icon_w = if ic.is_some() { 17.0 + 10.0 } else { 0.0 };
    let w = pad * 2.0 + icon_w + galley.size().x;
    let sense = if enabled { Sense::click() } else { Sense::hover() };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(w, h), sense);
    let fade = if enabled { 1.0 } else { 0.4 };
    let bg = if enabled && resp.hovered() { hover } else { fill };
    let r = CornerRadius::same(radius);
    ui.painter().rect_filled(rect, r, bg.gamma_multiply(fade));
    if border != Color32::TRANSPARENT {
        ui.painter().rect_stroke(rect, r, Stroke::new(1.0, border.gamma_multiply(fade)), egui::StrokeKind::Inside);
    }
    let mut x = rect.min.x + pad;
    if let Some(ic) = ic {
        icon(ui, Rect::from_min_size(Pos2::new(x, rect.center().y - 8.5), Vec2::splat(17.0)), ic, fg.gamma_multiply(fade), 2.3);
        x += icon_w;
    }
    ui.painter().galley(Pos2::new(x, rect.center().y - galley.size().y / 2.0), galley, fg.gamma_multiply(fade));
    if enabled {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    }
}

pub fn logo(ui: &mut Ui, size: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same((size * 0.25) as u8), ACCENT);
    let s = size / 32.0;
    for (i, alpha) in [255u8, 158, 77].into_iter().enumerate() {
        let y = 13.0 + 6.0 * i as f32;
        let pts = [rect.min + Vec2::new(10.0, y) * s, rect.min + Vec2::new(16.0, y - 5.5) * s, rect.min + Vec2::new(22.0, y) * s];
        let c = Color32::from_rgba_unmultiplied(0x1A, 0x0F, 0x05, alpha);
        p.line_segment([pts[0], pts[1]], Stroke::new(3.0 * s, c));
        p.line_segment([pts[1], pts[2]], Stroke::new(3.0 * s, c));
    }
}

pub fn nav_item(ui: &mut Ui, ic: Icon, label: &str, active: bool, badge: Option<usize>) {
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 40.0), Sense::click());
    let r = CornerRadius::same(10);
    if active {
        ui.painter().rect_filled(rect, r, Color32::from_rgba_unmultiplied(0xFF, 0x8A, 0x2A, 31));
    } else if resp.hovered() {
        ui.painter().rect_filled(rect, r, Color32::from_white_alpha(13));
    }
    let color = if active { ACCENT_TEXT } else if resp.hovered() { TEXT } else { MUTED };
    icon(ui, Rect::from_min_size(rect.min + Vec2::new(10.0, 11.0), Vec2::splat(18.0)), ic, color, 2.0);
    let font = if active { semibold() } else { egui::FontFamily::Proportional };
    ui.painter().text(rect.left_center() + Vec2::new(40.0, 0.0), Align2::LEFT_CENTER, label, FontId::new(14.0, font), color);
    if let Some(n) = badge.filter(|n| *n > 0) {
        let b = Rect::from_center_size(rect.right_center() - Vec2::new(21.0, 0.0), Vec2::new(22.0, 22.0));
        ui.painter().rect_filled(b, CornerRadius::same(11), ACCENT);
        ui.painter().text(b.center(), Align2::CENTER_CENTER, n.to_string(), FontId::new(12.0, semibold()), ON_ACCENT);
    }
}

pub fn tile(ui: &mut Ui, app: &App) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(40.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(11), tile_color(app.hue));
    let initial = app.name.chars().next().unwrap_or('?').to_uppercase().to_string();
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, initial, FontId::new(17.0, semibold()), Color32::WHITE);
}

pub fn chip(ui: &mut Ui, text: &str) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(86.0, 26.0), Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(7), Color32::from_white_alpha(13));
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, text, FontId::new(12.0, semibold()), TEXT_2);
}

pub fn progress_bar(ui: &mut Ui, frac: f32, h: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), Sense::hover());
    let r = CornerRadius::same((h / 2.0) as u8);
    ui.painter().rect_filled(rect, r, Color32::from_white_alpha(20));
    let mut fill = rect;
    fill.set_width(rect.width() * frac.clamp(0.0, 1.0));
    ui.painter().rect_filled(fill, r, ACCENT);
}
