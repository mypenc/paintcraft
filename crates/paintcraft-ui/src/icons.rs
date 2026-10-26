//! Icons drawn with painter primitives (no icon font ships with the app).

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, StrokeKind, pos2, vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Undo,
    Redo,
    Plus,
    Close,
    Check,
    Down,
    Up,
    Left,
    Right,
    Back,
    Layers,
    Brush,
    Eraser,
    Swap,
    Eye,
    EyeOff,
    Select,
    Hand,
    Shapes,
    Ruler,
    Image,
    Info,
    Play,
    Share,
    More,
    Trash,
    Copy,
    Folder,
    Gear,
    Camera,
    Pipette,
    Text,
    Bucket,
    Lasso,
    Wand,
    Move,
    Blur,
    Smudge,
    Filter,
    Canvas,
    FlipH,
    FlipV,
    Sliders,
    Merge,
    Menu,
    Refresh,
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n)
        .map(|i| {
            let t = a0 + (a1 - a0) * i as f32 / n as f32;
            pos2(c.x + r * t.cos(), c.y + r * t.sin())
        })
        .collect()
}

pub fn paint(p: &Painter, r: Rect, icon: Icon, color: Color32) {
    let c = r.center();
    let s = r.width().min(r.height()) * 0.5 * 0.78;
    let st = Stroke::new((s * 0.14).max(1.5), color);
    let line = |a: Pos2, b: Pos2| p.line_segment([a, b], st);
    let poly = |pts: Vec<Pos2>| {
        p.add(Shape::line(pts, st));
    };
    let at = |x: f32, y: f32| pos2(c.x + x * s, c.y + y * s);
    match icon {
        Icon::Undo | Icon::Redo => {
            let m = if icon == Icon::Undo { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = arc(pos2(c.x, c.y + s * 0.1), s * 0.7, 0.0, -std::f32::consts::PI, 16).into_iter().map(|q| pos2(c.x + (q.x - c.x) * m, q.y)).collect();
            let end = pts.last().copied().unwrap_or(c);
            poly(pts);
            line(end, pos2(end.x - 0.4 * s, end.y - 0.4 * s));
            line(end, pos2(end.x + 0.4 * s, end.y - 0.4 * s));
        }
        Icon::Plus => {
            line(at(-0.8, 0.0), at(0.8, 0.0));
            line(at(0.0, -0.8), at(0.0, 0.8));
        }
        Icon::Close => {
            line(at(-0.7, -0.7), at(0.7, 0.7));
            line(at(-0.7, 0.7), at(0.7, -0.7));
        }
        Icon::Check => poly(vec![at(-0.8, 0.05), at(-0.25, 0.6), at(0.85, -0.6)]),
        Icon::Down => poly(vec![at(-0.7, -0.3), at(0.0, 0.4), at(0.7, -0.3)]),
        Icon::Up => poly(vec![at(-0.7, 0.3), at(0.0, -0.4), at(0.7, 0.3)]),
        Icon::Left => poly(vec![at(0.3, -0.7), at(-0.4, 0.0), at(0.3, 0.7)]),
        Icon::Right => poly(vec![at(-0.3, -0.7), at(0.4, 0.0), at(-0.3, 0.7)]),
        Icon::Back => {
            line(at(-0.85, 0.0), at(0.85, 0.0));
            poly(vec![at(-0.3, -0.55), at(-0.85, 0.0), at(-0.3, 0.55)]);
        }
        Icon::Layers => {
            for dy in [0.35, -0.15] {
                p.add(Shape::closed_line(vec![at(0.0, dy - 0.5), at(0.85, dy), at(0.0, dy + 0.5), at(-0.85, dy)], st));
            }
        }
        Icon::Brush => {
            p.line_segment([at(0.75, -0.75), at(-0.2, 0.2)], Stroke::new(s * 0.32, color));
            p.circle_filled(at(-0.45, 0.5), s * 0.34, color);
        }
        Icon::Eraser => {
            p.add(Shape::convex_polygon(vec![at(-0.2, -0.8), at(0.85, 0.1), at(0.25, 0.7), at(-0.8, -0.2)], Color32::TRANSPARENT, st));
            line(at(-0.5, 0.25), at(0.55, -0.4));
            line(at(-0.1, 0.8), at(0.85, 0.8));
        }
        Icon::Swap => {
            p.line_segment([at(0.8, -0.8), at(-0.1, 0.1)], Stroke::new(s * 0.3, color));
            p.circle_filled(at(-0.4, 0.5), s * 0.3, color);
            line(at(-0.9, -0.55), at(-0.5, -0.55));
            line(at(-0.7, -0.75), at(-0.5, -0.55));
            line(at(-0.7, -0.35), at(-0.5, -0.55));
        }
        Icon::Eye | Icon::EyeOff => {
            let top = arc(pos2(c.x, c.y + s * 0.6), s * 1.0, -2.4, -0.74, 14);
            let bot = arc(pos2(c.x, c.y - s * 0.6), s * 1.0, 0.74, 2.4, 14);
            let mut pts = top;
            pts.extend(bot);
            p.add(Shape::closed_line(pts, st));
            if icon == Icon::Eye {
                p.circle_filled(c, s * 0.28, color);
            } else {
                line(at(-0.8, 0.8), at(0.8, -0.8));
            }
        }
        Icon::Select => {
            let q = [(-0.8, -0.8, 0.8, -0.8), (0.8, -0.8, 0.8, 0.8), (0.8, 0.8, -0.8, 0.8), (-0.8, 0.8, -0.8, -0.8)];
            for (x0, y0, x1, y1) in q {
                let (a, b) = (at(x0, y0), at(x1, y1));
                for k in 0..4 {
                    let t0 = k as f32 / 4.0 + 0.04;
                    let t1 = k as f32 / 4.0 + 0.16;
                    line(a + (b - a) * t0, a + (b - a) * t1);
                }
            }
        }
        Icon::Hand => {
            p.add(Shape::closed_line(vec![at(-0.55, 0.1), at(-0.55, 0.7), at(0.5, 0.7), at(0.5, 0.0)], st));
            for (i, x) in [-0.45f32, -0.15, 0.15, 0.45].iter().enumerate() {
                let top = -0.7 + (i as f32 % 3.0) * 0.08;
                line(at(*x, 0.1), at(*x, top));
            }
            line(at(-0.55, 0.1), at(-0.85, -0.2));
        }
        Icon::Shapes => {
            p.rect_stroke(Rect::from_min_max(at(-0.85, -0.85), at(0.2, 0.2)), 2.0, st, StrokeKind::Middle);
            p.circle_stroke(at(0.3, 0.3), s * 0.55, st);
        }
        Icon::Ruler => {
            p.add(Shape::convex_polygon(vec![at(0.85, -0.45), at(0.45, -0.85), at(-0.85, 0.45), at(-0.45, 0.85)], Color32::TRANSPARENT, st));
            p.circle_stroke(at(-0.1, 0.1), s * 0.14, st);
        }
        Icon::Image => {
            p.rect_stroke(Rect::from_min_max(at(-0.85, -0.7), at(0.85, 0.7)), 2.0, st, StrokeKind::Middle);
            poly(vec![at(-0.85, 0.5), at(-0.3, -0.1), at(0.1, 0.35), at(0.4, 0.05), at(0.85, 0.55)]);
            p.circle_filled(at(0.4, -0.35), s * 0.14, color);
        }
        Icon::Info => {
            p.circle_stroke(c, s * 0.9, st);
            p.circle_filled(at(0.0, -0.4), s * 0.1, color);
            line(at(0.0, -0.05), at(0.0, 0.55));
        }
        Icon::Play => {
            p.add(Shape::convex_polygon(vec![at(-0.5, -0.75), at(0.8, 0.0), at(-0.5, 0.75)], color, Stroke::NONE));
        }
        Icon::Share => {
            let (a, b, d) = (at(0.6, -0.6), at(-0.6, 0.0), at(0.6, 0.6));
            line(a, b);
            line(b, d);
            for q in [a, b, d] {
                p.circle_filled(q, s * 0.22, color);
            }
        }
        Icon::More => {
            for y in [-0.6, 0.0, 0.6] {
                p.circle_filled(at(0.0, y), s * 0.14, color);
            }
        }
        Icon::Menu => {
            for y in [-0.55, 0.0, 0.55] {
                line(at(-0.75, y), at(0.75, y));
            }
        }
        Icon::Trash => {
            line(at(-0.8, -0.55), at(0.8, -0.55));
            line(at(-0.3, -0.55), at(-0.3, -0.8));
            line(at(0.3, -0.55), at(0.3, -0.8));
            line(at(-0.3, -0.8), at(0.3, -0.8));
            p.add(Shape::line(vec![at(-0.6, -0.4), at(-0.5, 0.85), at(0.5, 0.85), at(0.6, -0.4)], st));
        }
        Icon::Copy => {
            p.rect_stroke(Rect::from_min_max(at(-0.3, -0.3), at(0.8, 0.8)), 2.0, st, StrokeKind::Middle);
            poly(vec![at(-0.3, 0.45), at(-0.8, 0.45), at(-0.8, -0.8), at(0.45, -0.8), at(0.45, -0.3)]);
        }
        Icon::Folder => {
            p.add(Shape::closed_line(vec![at(-0.85, -0.6), at(-0.25, -0.6), at(0.0, -0.3), at(0.85, -0.3), at(0.85, 0.65), at(-0.85, 0.65)], st));
        }
        Icon::Gear => {
            p.circle_stroke(c, s * 0.38, st);
            for k in 0..8 {
                let a = k as f32 * std::f32::consts::FRAC_PI_4;
                line(pos2(c.x + a.cos() * s * 0.6, c.y + a.sin() * s * 0.6), pos2(c.x + a.cos() * s * 0.9, c.y + a.sin() * s * 0.9));
            }
            p.circle_stroke(c, s * 0.62, st);
        }
        Icon::Camera => {
            p.rect_stroke(Rect::from_min_max(at(-0.9, -0.5), at(0.9, 0.7)), 3.0, st, StrokeKind::Middle);
            line(at(-0.4, -0.5), at(-0.2, -0.8));
            line(at(-0.2, -0.8), at(0.2, -0.8));
            line(at(0.2, -0.8), at(0.4, -0.5));
            p.circle_stroke(at(0.0, 0.1), s * 0.3, st);
        }
        Icon::Pipette => {
            line(at(0.8, -0.8), at(0.2, -0.2));
            line(at(0.2, -0.2), at(-0.7, 0.7));
            p.circle_filled(at(0.55, -0.55), s * 0.2, color);
        }
        Icon::Text => {
            p.text(c, Align2::CENTER_CENTER, "T", FontId::proportional(s * 2.0), color);
        }
        Icon::Bucket => {
            p.add(Shape::closed_line(vec![at(-0.2, -0.8), at(0.8, 0.0), at(0.1, 0.7), at(-0.85, -0.1)], st));
            p.circle_filled(at(-0.8, 0.55), s * 0.18, color);
        }
        Icon::Lasso => {
            poly(arc(at(0.0, -0.2), s * 0.75, 0.0, std::f32::consts::TAU, 24));
            poly(vec![at(-0.5, 0.4), at(-0.7, 0.6), at(-0.5, 0.85)]);
        }
        Icon::Wand => {
            line(at(0.8, -0.8), at(-0.7, 0.7));
            line(at(-0.5, -0.8), at(-0.5, -0.4));
            line(at(-0.7, -0.6), at(-0.3, -0.6));
            line(at(0.0, 0.3), at(0.0, 0.7));
            line(at(-0.2, 0.5), at(0.2, 0.5));
        }
        Icon::Move => {
            line(at(-0.85, 0.0), at(0.85, 0.0));
            line(at(0.0, -0.85), at(0.0, 0.85));
            poly(vec![at(-0.6, -0.25), at(-0.85, 0.0), at(-0.6, 0.25)]);
            poly(vec![at(0.6, -0.25), at(0.85, 0.0), at(0.6, 0.25)]);
            poly(vec![at(-0.25, -0.6), at(0.0, -0.85), at(0.25, -0.6)]);
            poly(vec![at(-0.25, 0.6), at(0.0, 0.85), at(0.25, 0.6)]);
        }
        Icon::Blur => {
            p.add(Shape::closed_line(vec![at(0.0, -0.9), at(0.55, 0.1), at(0.45, 0.55), at(0.0, 0.85), at(-0.45, 0.55), at(-0.55, 0.1)], st));
        }
        Icon::Smudge => {
            poly(vec![at(-0.8, 0.6), at(-0.2, 0.3), at(0.3, 0.45), at(0.8, -0.1)]);
            p.circle_filled(at(0.8, -0.1), s * 0.2, color);
        }
        Icon::Filter => {
            p.circle_stroke(c, s * 0.9, st);
            p.text(c, Align2::CENTER_CENTER, "FX", FontId::proportional(s * 0.95), color);
        }
        Icon::Canvas => {
            p.rect_stroke(Rect::from_min_max(at(-0.85, -0.85), at(0.85, 0.85)), 1.0, st, StrokeKind::Middle);
            p.rect_stroke(Rect::from_min_max(at(-0.45, -0.45), at(0.45, 0.45)), 1.0, st, StrokeKind::Middle);
        }
        Icon::FlipH => {
            line(at(0.0, -0.85), at(0.0, 0.85));
            p.add(Shape::closed_line(vec![at(-0.2, -0.6), at(-0.85, 0.6), at(-0.2, 0.6)], st));
            p.add(Shape::closed_line(vec![at(0.2, -0.6), at(0.85, 0.6), at(0.2, 0.6)], st));
        }
        Icon::FlipV => {
            line(at(-0.85, 0.0), at(0.85, 0.0));
            p.add(Shape::closed_line(vec![at(-0.6, -0.2), at(0.6, -0.85), at(0.6, -0.2)], st));
            p.add(Shape::closed_line(vec![at(-0.6, 0.2), at(0.6, 0.85), at(0.6, 0.2)], st));
        }
        Icon::Sliders => {
            for (y, x) in [(-0.5, -0.3), (0.5, 0.35)] {
                line(at(-0.85, y), at(0.85, y));
                p.circle_filled(at(x, y), s * 0.26, color);
            }
        }
        Icon::Merge => {
            line(at(0.0, -0.85), at(0.0, 0.2));
            poly(vec![at(-0.5, -0.25), at(0.0, 0.25), at(0.5, -0.25)]);
            line(at(-0.8, 0.75), at(0.8, 0.75));
        }
        Icon::Refresh => {
            poly(arc(c, s * 0.7, -0.5, 4.6, 20));
            let e = pos2(c.x + s * 0.7 * (-0.5f32).cos(), c.y + s * 0.7 * (-0.5f32).sin());
            line(e, pos2(e.x + 0.4 * s, e.y - 0.1 * s));
            line(e, pos2(e.x + 0.05 * s, e.y + 0.45 * s));
        }
    }
    let _ = vec2(0.0, 0.0);
}
