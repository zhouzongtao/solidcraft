//! A registered parameter figure ([`solidcraft_engine::extensions::Figure`]) drawn to fit a
//! rectangle: the outline in the text colour, dimensions dimmed, and the groups of the focused
//! field in the accent colour.

use egui::{Color32, FontId, Pos2, Rect, Shape, Stroke, pos2, vec2};
use solidcraft_engine::extensions::{Figure, HAlign, Item, VAlign};

use crate::theme::Tokens;

/// Draw `f` into `rect`, lighting the groups named in `lit`.
pub fn paint(painter: &egui::Painter, rect: Rect, f: &Figure, lit: &[&str]) {
    let t = Tokens::get();
    let [x0, y0, x1, y1] = f.bounds;
    let inner = rect.shrink(6.0);
    let scale = (f64::from(inner.width()) / (x1 - x0)).min(f64::from(inner.height()) / (y1 - y0));
    if !(scale.is_finite() && scale > 0.0) {
        return;
    }
    let (w, h) = (((x1 - x0) * scale) as f32, ((y1 - y0) * scale) as f32);
    let origin = pos2(inner.center().x - w / 2.0, inner.center().y + h / 2.0);
    let at = |p: [f64; 2]| pos2(origin.x + ((p[0] - x0) * scale) as f32, origin.y - ((p[1] - y0) * scale) as f32);
    // Lit groups last, so they draw over what they cross.
    let mut groups: Vec<_> = f.groups.iter().map(|g| (lit.contains(&g.name.as_str()), g)).collect();
    groups.sort_by_key(|(on, _)| *on);
    for (on, g) in groups {
        let colour = if on {
            t.accent
        } else if g.name.is_empty() {
            t.text
        } else {
            t.text_dim
        };
        let stroke = Stroke::new(if on { 1.8 } else { 1.0 }, colour);
        for item in &g.items {
            draw(painter, item, &at, scale, stroke);
        }
    }
}

fn draw(painter: &egui::Painter, item: &Item, at: &dyn Fn([f64; 2]) -> Pos2, scale: f64, stroke: Stroke) {
    match item {
        Item::Line { a, b } => {
            painter.line_segment([at(*a), at(*b)], stroke);
        }
        Item::Polyline { points, closed, filled } => {
            let pts: Vec<Pos2> = points.iter().map(|p| at(*p)).collect();
            if *filled {
                painter.add(Shape::convex_polygon(pts, stroke.color, Stroke::NONE));
            } else if *closed {
                painter.add(Shape::closed_line(pts, stroke));
            } else {
                painter.add(Shape::line(pts, stroke));
            }
        }
        Item::Arc { center, radius, start, end } => {
            let sweep = (end - start).rem_euclid(std::f64::consts::TAU);
            let sweep = if sweep < 1e-9 { std::f64::consts::TAU } else { sweep };
            let n = ((sweep / std::f64::consts::TAU * 64.0).ceil() as usize).clamp(4, 64);
            let pts = (0..=n)
                .map(|i| {
                    let a = start + sweep * i as f64 / n as f64;
                    at([center[0] + radius * a.cos(), center[1] + radius * a.sin()])
                })
                .collect();
            painter.add(Shape::line(pts, stroke));
        }
        Item::Circle { center, radius } => {
            painter.circle_stroke(at(*center), (radius * scale) as f32, stroke);
        }
        Item::Text { at: p, text, height, angle, h, v } => {
            // Cap height to font size; too small to read is drawn at the smallest readable size.
            let size = ((height * scale) as f32 * 1.4).clamp(6.0, 48.0);
            let galley = painter.layout_no_wrap(text.clone(), FontId::proportional(size), stroke.color);
            let anchor = egui::Align2([
                match h {
                    HAlign::Left => egui::Align::Min,
                    HAlign::Center => egui::Align::Center,
                    HAlign::Right => egui::Align::Max,
                },
                match v {
                    VAlign::Top => egui::Align::Min,
                    VAlign::Middle => egui::Align::Center,
                    VAlign::Bottom => egui::Align::Max,
                },
            ]);
            let corner = anchor.anchor_size(at(*p), galley.size()).min;
            let shape = egui::epaint::TextShape::new(corner, galley, Color32::PLACEHOLDER).with_angle_and_anchor(-(*angle as f32), anchor);
            painter.add(shape);
        }
    }
}

/// A figure's size in a dialog: the given width, its own proportions, at most 3:2 tall.
pub fn size(f: &Figure, width: f32) -> egui::Vec2 {
    let [x0, y0, x1, y1] = f.bounds;
    let aspect = ((y1 - y0) / (x1 - x0)) as f32;
    vec2(width, (width * aspect).clamp(60.0, width * 1.5))
}
