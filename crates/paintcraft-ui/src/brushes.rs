//! The brush list: each brush is a patch of the engine's brush settings plus a default size.

use egui::{Color32, Painter, Rect, pos2};
use serde_json::{Value, json};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Category {
    All,
    Pen,
    Sketch,
    Ink,
    Airbrush,
    Paint,
    Marker,
}

impl Category {
    pub const LIST: [Category; 7] = [Category::All, Category::Pen, Category::Sketch, Category::Ink, Category::Airbrush, Category::Paint, Category::Marker];

    pub fn label(self) -> &'static str {
        match self {
            Category::All => "All",
            Category::Pen => "Pens",
            Category::Sketch => "Sketch",
            Category::Ink => "Ink",
            Category::Airbrush => "Airbrush",
            Category::Paint => "Paint",
            Category::Marker => "Marker",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct BrushDef {
    pub name: &'static str,
    pub category: Category,
    pub hardness: f32,
    pub spacing: f32,
    pub flow: f32,
    pub size: f32,
    /// Opacity the brush starts with when picked.
    pub opacity: f32,
}

pub const BRUSHES: &[BrushDef] = &[
    BrushDef { name: "Dip Pen (Smooth)", category: Category::Pen, hardness: 0.85, spacing: 0.08, flow: 1.0, size: 12.0, opacity: 1.0 },
    BrushDef { name: "Dip Pen (Hard)", category: Category::Pen, hardness: 1.0, spacing: 0.07, flow: 1.0, size: 10.0, opacity: 1.0 },
    BrushDef { name: "Felt Tip Pen", category: Category::Pen, hardness: 0.9, spacing: 0.1, flow: 1.0, size: 16.0, opacity: 1.0 },
    BrushDef { name: "Digital Pen", category: Category::Pen, hardness: 1.0, spacing: 0.05, flow: 1.0, size: 14.0, opacity: 1.0 },
    BrushDef { name: "Soft Pen", category: Category::Pen, hardness: 0.55, spacing: 0.08, flow: 1.0, size: 20.0, opacity: 1.0 },
    BrushDef { name: "Pencil", category: Category::Sketch, hardness: 0.6, spacing: 0.12, flow: 0.45, size: 9.0, opacity: 0.9 },
    BrushDef { name: "Soft Pencil", category: Category::Sketch, hardness: 0.35, spacing: 0.12, flow: 0.3, size: 14.0, opacity: 0.85 },
    BrushDef { name: "Charcoal", category: Category::Sketch, hardness: 0.25, spacing: 0.2, flow: 0.35, size: 40.0, opacity: 0.9 },
    BrushDef { name: "G-Pen", category: Category::Ink, hardness: 1.0, spacing: 0.04, flow: 1.0, size: 8.0, opacity: 1.0 },
    BrushDef { name: "Maru Pen", category: Category::Ink, hardness: 1.0, spacing: 0.04, flow: 1.0, size: 5.0, opacity: 1.0 },
    BrushDef { name: "Airbrush (Soft)", category: Category::Airbrush, hardness: 0.0, spacing: 0.1, flow: 0.5, size: 80.0, opacity: 0.8 },
    BrushDef { name: "Airbrush (Medium)", category: Category::Airbrush, hardness: 0.25, spacing: 0.1, flow: 0.6, size: 60.0, opacity: 0.9 },
    BrushDef { name: "Watercolor", category: Category::Paint, hardness: 0.3, spacing: 0.1, flow: 0.25, size: 70.0, opacity: 0.7 },
    BrushDef { name: "Flat Brush", category: Category::Paint, hardness: 0.7, spacing: 0.08, flow: 0.8, size: 60.0, opacity: 1.0 },
    BrushDef { name: "Marker", category: Category::Marker, hardness: 0.95, spacing: 0.1, flow: 0.6, size: 30.0, opacity: 0.8 },
    BrushDef { name: "Highlighter", category: Category::Marker, hardness: 1.0, spacing: 0.1, flow: 0.35, size: 36.0, opacity: 0.5 },
];

impl BrushDef {
    /// The `brush` patch for `paint.stroke`.
    pub fn patch(&self) -> Value {
        json!({"hardness": self.hardness, "spacing": self.spacing, "flow": self.flow})
    }
}

/// Draw a brush's sample stroke (an S-curve), thick and soft as the brush is.
pub fn preview(p: &Painter, rect: Rect, b: &BrushDef, color: Color32) {
    let n = 48;
    let r = (b.size.sqrt() * 0.9).clamp(1.5, rect.height() * 0.32);
    let alpha = |a: f32| Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), (a.clamp(0.0, 1.0) * 255.0) as u8);
    for i in 0..=n {
        let t = i as f32 / n as f32;
        let x = rect.left() + rect.width() * (0.06 + 0.88 * t);
        let y = rect.center().y + (t * std::f32::consts::TAU).sin() * rect.height() * 0.2;
        let c = pos2(x, y);
        let wob = 1.0 - ((t - 0.5).abs() * 1.2);
        let rr = r * (0.45 + 0.55 * wob.clamp(0.0, 1.0));
        if b.hardness >= 0.8 {
            p.circle_filled(c, rr, alpha(b.opacity.max(0.6)));
        } else {
            for (k, scale) in [(1.0f32, 1.0f32), (0.7, 0.7), (0.4, 0.45)] {
                p.circle_filled(c, rr * scale * 1.2, alpha(0.12 + (1.0 - b.hardness) * 0.1 * k));
            }
        }
    }
}
