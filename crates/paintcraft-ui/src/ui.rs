//! Shared look and touch-sized widgets: the dark-gray ibisPaint-like palette, round and text
//! buttons, a finger-friendly slider, text dialogs and toasts.

use std::sync::Mutex;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use egui::{Align2, Color32, Context, FontId, Id, Margin, Rect, Response, Sense, Stroke, StrokeKind, TextStyle, Ui, Vec2, pos2, vec2};

use crate::icons::{self, Icon};
use crate::i18n::t;

pub const BAR: Color32 = Color32::from_rgb(0x1b, 0x1b, 0x1d);
pub const PANEL: Color32 = Color32::from_rgb(0x66, 0x66, 0x68);
pub const PANEL_DARK: Color32 = Color32::from_rgb(0x4b, 0x4b, 0x4d);
pub const PANEL_DEEP: Color32 = Color32::from_rgb(0x2c, 0x2c, 0x2e);
pub const SIDEBAR: Color32 = Color32::from_rgb(0x12, 0x12, 0x13);
pub const ACCENT: Color32 = Color32::from_rgb(0x4f, 0x93, 0xf0);
pub const CANVAS_BG: Color32 = Color32::from_rgb(0xbd, 0xbd, 0xbd);
pub const GALLERY_BG: Color32 = Color32::from_rgb(0xf3, 0xf1, 0xe4);
pub const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf2, 0xf2);
pub const TEXT_DIM: Color32 = Color32::from_rgb(0xb4, 0xb4, 0xb6);
pub const DANGER: Color32 = Color32::from_rgb(0xe5, 0x39, 0x35);
pub const SHEET: Color32 = Color32::from_rgb(0xe6, 0xe6, 0xe6);

pub fn setup(ctx: &Context) {
    ctx.global_style_mut(|s| {
        s.visuals = egui::Visuals::dark();
        s.visuals.panel_fill = BAR;
        s.visuals.window_fill = PANEL_DARK;
        s.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.6);
        s.visuals.override_text_color = None;
        s.spacing.item_spacing = vec2(8.0, 8.0);
        s.spacing.interact_size = vec2(40.0, 40.0);
        s.spacing.button_padding = vec2(12.0, 8.0);
        s.text_styles.insert(TextStyle::Body, FontId::proportional(16.0));
        s.text_styles.insert(TextStyle::Button, FontId::proportional(16.0));
        s.text_styles.insert(TextStyle::Heading, FontId::proportional(20.0));
        s.text_styles.insert(TextStyle::Small, FontId::proportional(12.5));
        s.text_styles.insert(TextStyle::Monospace, FontId::monospace(15.0));
    });
}

/// A round (or rounded) icon button. `fill` is the disc colour.
pub fn icon_button(ui: &mut Ui, size: f32, icon: Icon, fill: Color32, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { fill.gamma_multiply(0.7) } else { fill };
    ui.painter().circle_filled(rect.center(), size * 0.5, fill);
    icons::paint(ui.painter(), rect.shrink(size * 0.2), icon, color);
    resp
}

/// A bare icon (no disc) that still has a touch-sized hit area.
pub fn bare_icon(ui: &mut Ui, size: f32, icon: Icon, color: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(size, size), Sense::click());
    let color = if resp.is_pointer_button_down_on() { color.gamma_multiply(0.6) } else { color };
    icons::paint(ui.painter(), rect.shrink(size * 0.2), icon, color);
    resp
}

/// A rounded text button.
pub fn text_button(ui: &mut Ui, label: &str, fill: Color32, text: Color32, min_w: f32) -> Response {
    let font = FontId::proportional(16.0);
    let galley = ui.painter().layout_no_wrap(label.to_owned(), font, text);
    let w = (galley.size().x + 28.0).max(min_w);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 44.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { fill.gamma_multiply(0.75) } else { fill };
    ui.painter().rect_filled(rect, 10.0, fill);
    let pos = rect.center() - galley.size() * 0.5;
    ui.painter().galley(pos, galley, text);
    resp
}

/// A full-width list row with a left label and optional right value; returns the click.
pub fn row(ui: &mut Ui, label: &str, right: &str, danger: bool, bg: Color32, fg: Color32) -> Response {
    let w = ui.available_width();
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 54.0), Sense::click());
    let fill = if resp.is_pointer_button_down_on() { bg.gamma_multiply(0.85) } else { bg };
    ui.painter().rect_filled(rect, 0.0, fill);
    let col = if danger { DANGER } else { fg };
    ui.painter().text(pos2(rect.left() + 16.0, rect.center().y), Align2::LEFT_CENTER, label, FontId::proportional(17.0), col);
    if !right.is_empty() {
        ui.painter().text(pos2(rect.right() - 16.0, rect.center().y), Align2::RIGHT_CENTER, right, FontId::proportional(16.0), fg.gamma_multiply(0.7));
    }
    ui.painter().line_segment([rect.left_bottom(), rect.right_bottom()], Stroke::new(1.0, Color32::from_black_alpha(40)));
    resp
}

/// A finger-friendly horizontal slider. Returns true while the value changes.
/// `log`: positions map exponentially between `min` and `max` (brush sizes).
pub fn slider_ex(ui: &mut Ui, width: f32, value: &mut f32, min: f32, max: f32, log: bool) -> (bool, bool) {
    let h = 40.0;
    let (rect, resp) = ui.allocate_exact_size(vec2(width, h), Sense::click_and_drag());
    let track = Rect::from_min_max(pos2(rect.left() + 16.0, rect.center().y - 3.0), pos2(rect.right() - 16.0, rect.center().y + 3.0));
    let to_t = |v: f32| -> f32 {
        if log { ((v.max(min) / min).ln() / (max / min).ln()).clamp(0.0, 1.0) } else { ((v - min) / (max - min)).clamp(0.0, 1.0) }
    };
    let mut changed = false;
    if resp.dragged() || resp.is_pointer_button_down_on() {
        if let Some(pos) = resp.interact_pointer_pos() {
            let t = ((pos.x - track.left()) / track.width().max(1.0)).clamp(0.0, 1.0);
            let v = if log { min * (max / min).powf(t) } else { min + (max - min) * t };
            if (v - *value).abs() > f32::EPSILON {
                *value = v;
                changed = true;
            }
        }
    }
    let t = to_t(*value);
    let painter = ui.painter();
    painter.rect_filled(track, 3.0, Color32::from_gray(0x3a));
    let filled = Rect::from_min_max(track.min, pos2(track.left() + track.width() * t, track.max.y));
    painter.rect_filled(filled, 3.0, Color32::from_rgb(0x3d, 0x6f, 0xb8));
    let knob = pos2(track.left() + track.width() * t, rect.center().y);
    painter.circle_filled(knob, 15.0, Color32::from_gray(0xf4));
    painter.circle_stroke(knob, 15.0, Stroke::new(1.0, Color32::from_black_alpha(80)));
    (changed, resp.drag_stopped() || resp.clicked())
}

/// [`slider_ex`] without the release flag.
pub fn slider(ui: &mut Ui, width: f32, value: &mut f32, min: f32, max: f32, log: bool) -> bool {
    slider_ex(ui, width, value, min, max, log).0
}

/// A slider row: "−  label value  +" around the slider, like the canvas' size and opacity bars.
pub fn stepper_slider(ui: &mut Ui, _id: &str, label: &str, value: &mut f32, min: f32, max: f32, log: bool, step: f32, decimals: usize) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_sized([62.0, 34.0], egui::Label::new(egui::RichText::new(label).color(TEXT).size(15.0)));
        if round_step(ui, "−") {
            *value = (*value - step).max(min);
            changed = true;
        }
        let w = (ui.available_width() - 48.0).max(60.0);
        changed |= slider(ui, w, value, min, max, log);
        if round_step(ui, "+") {
            *value = (*value + step).min(max);
            changed = true;
        }
    });
    let _ = decimals;
    changed
}

fn round_step(ui: &mut Ui, glyph: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::click());
    ui.painter().circle_filled(rect.center(), 18.0, Color32::BLACK);
    ui.painter().text(rect.center(), Align2::CENTER_CENTER, glyph, FontId::proportional(22.0), Color32::WHITE);
    resp.clicked()
}

/// A rounded card frame for panels and dialogs.
pub fn card(fill: Color32) -> egui::Frame {
    egui::Frame::new().fill(fill).corner_radius(14.0).inner_margin(Margin::same(10))
}

pub fn checker(painter: &egui::Painter, rect: Rect, cell: f32) {
    let mut mesh = egui::epaint::Mesh::default();
    let (a, b) = (Color32::from_gray(0xff), Color32::from_gray(0xd6));
    let (cols, rows) = (((rect.width() / cell).ceil() as i32).clamp(1, 400), ((rect.height() / cell).ceil() as i32).clamp(1, 400));
    for y in 0..rows {
        for x in 0..cols {
            let min = pos2(rect.left() + x as f32 * cell, rect.top() + y as f32 * cell);
            let max = pos2((min.x + cell).min(rect.right()), (min.y + cell).min(rect.bottom()));
            mesh.add_colored_rect(Rect::from_min_max(min, max), if (x + y) % 2 == 0 { a } else { b });
        }
    }
    painter.add(egui::Shape::mesh(mesh));
}

// ---- toasts ------------------------------------------------------------------------------------

static TOASTS: Mutex<Vec<(String, Instant)>> = Mutex::new(Vec::new());

/// Show a short message at the bottom of the screen.
pub fn toast(msg: impl Into<String>) {
    TOASTS.lock().unwrap_or_else(PoisonError::into_inner).push((msg.into(), Instant::now()));
}

pub fn show_toasts(ctx: &Context) {
    let mut toasts = TOASTS.lock().unwrap_or_else(PoisonError::into_inner);
    toasts.retain(|(_, at)| at.elapsed() < Duration::from_millis(2600));
    if toasts.is_empty() {
        return;
    }
    ctx.request_repaint_after(Duration::from_millis(200));
    let screen = ctx.content_rect();
    for (i, (msg, _)) in toasts.iter().enumerate() {
        egui::Area::new(Id::new(("toast", i))).order(egui::Order::Tooltip).fixed_pos(pos2(screen.center().x, screen.bottom() - 130.0 - i as f32 * 52.0)).pivot(Align2::CENTER_CENTER).interactable(false).show(ctx, |ui| {
            egui::Frame::new().fill(Color32::from_black_alpha(220)).corner_radius(20.0).inner_margin(Margin::symmetric(18, 10)).show(ui, |ui| {
                ui.label(egui::RichText::new(msg).color(Color32::WHITE).size(15.0));
            });
        });
    }
}

// ---- dialogs -----------------------------------------------------------------------------------

pub enum DialogResult {
    Open,
    Ok,
    Cancel,
}

/// A dialog with a single text field. `text` is edited in place.
pub fn text_dialog(ctx: &Context, id: &str, title: &str, text: &mut String, ok_label: &str) -> DialogResult {
    let mut result = DialogResult::Open;
    let width = (ctx.content_rect().width() - 48.0).min(460.0);
    egui::Modal::new(Id::new(id)).show(ctx, |ui| {
        ui.set_width(width);
        ui.label(egui::RichText::new(title).size(19.0).strong());
        ui.add_space(6.0);
        let edit = egui::TextEdit::singleline(text).desired_width(f32::INFINITY).font(FontId::proportional(18.0)).margin(Margin::symmetric(10, 10));
        ui.add(edit);
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if text_button(ui, &t("Cancel"), PANEL, TEXT, 100.0).clicked() {
                result = DialogResult::Cancel;
            }
            if text_button(ui, ok_label, ACCENT, Color32::WHITE, 100.0).clicked() {
                result = DialogResult::Ok;
            }
        });
    });
    result
}

/// A yes/no confirmation.
pub fn confirm_dialog(ctx: &Context, id: &str, title: &str, body: &str, ok_label: &str, danger: bool) -> DialogResult {
    let mut result = DialogResult::Open;
    let width = (ctx.content_rect().width() - 48.0).min(460.0);
    egui::Modal::new(Id::new(id)).show(ctx, |ui| {
        ui.set_width(width);
        ui.label(egui::RichText::new(title).size(19.0).strong());
        ui.add_space(6.0);
        ui.label(egui::RichText::new(body).size(16.0));
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if text_button(ui, &t("Cancel"), PANEL, TEXT, 100.0).clicked() {
                result = DialogResult::Cancel;
            }
            if text_button(ui, ok_label, if danger { DANGER } else { ACCENT }, Color32::WHITE, 100.0).clicked() {
                result = DialogResult::Ok;
            }
        });
    });
    result
}

/// A bottom-anchored (or caller-positioned) popup list used for the ⋮ and tool menus.
pub fn popup_pos(ctx: &Context, anchor: Align2, offset: Vec2) -> egui::Pos2 {
    let r = ctx.content_rect();
    let x = match anchor.x() {
        egui::Align::Min => r.left() + offset.x,
        egui::Align::Center => r.center().x + offset.x,
        egui::Align::Max => r.right() + offset.x,
    };
    let y = match anchor.y() {
        egui::Align::Min => r.top() + offset.y,
        egui::Align::Center => r.center().y + offset.y,
        egui::Align::Max => r.bottom() + offset.y,
    };
    pos2(x, y)
}

pub fn stroke_rect(p: &egui::Painter, r: Rect, radius: f32, color: Color32, w: f32) {
    p.rect_stroke(r, radius, Stroke::new(w, color), StrokeKind::Inside);
}
