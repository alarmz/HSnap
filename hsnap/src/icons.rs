//! Tool-palette glyphs, drawn with egui primitives (no bitmap assets).

use crate::app::Tool;
use eframe::egui::{self, Color32, Pos2, Rect, Stroke, Ui, Vec2, epaint::EllipseShape};

/// A square palette button with a glyph for `tool`. Returns the response.
pub fn tool_button(ui: &mut Ui, tool: Tool, selected: bool, hint: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::click());
    let vis = ui.style().interact_selectable(&resp, selected);
    ui.painter().rect(rect.shrink(1.0), 3.0, if selected { vis.bg_fill } else { Color32::TRANSPARENT }, vis.bg_stroke, egui::StrokeKind::Inside);
    paint(ui, rect.shrink(6.0), tool, ui.visuals().text_color());
    resp.on_hover_text(hint)
}

fn paint(ui: &Ui, r: Rect, tool: Tool, c: Color32) {
    let p = ui.painter();
    let st = Stroke::new(1.5, c);
    let (l, t, rt, b) = (r.left(), r.top(), r.right(), r.bottom());
    let at = |fx: f32, fy: f32| Pos2::new(l + r.width() * fx, t + r.height() * fy);
    match tool {
        Tool::Select => {
            // dotted marquee
            for i in 0..6 {
                let f = i as f32 / 6.0;
                for (a, bb) in [(at(f, 0.0), at(f + 0.08, 0.0)), (at(f, 1.0), at(f + 0.08, 1.0)), (at(0.0, f), at(0.0, f + 0.08)), (at(1.0, f), at(1.0, f + 0.08))] {
                    p.line_segment([a, bb], st);
                }
            }
        }
        Tool::FloodFill => {
            p.add(egui::Shape::convex_polygon(vec![at(0.1, 0.45), at(0.5, 0.05), at(0.9, 0.45), at(0.5, 0.85)], c.gamma_multiply(0.35), st));
            p.circle_filled(at(0.88, 0.8), 2.5, c);
        }
        Tool::Text => {
            p.text(r.center(), egui::Align2::CENTER_CENTER, "A", egui::FontId::proportional(r.height()), c);
        }
        Tool::Airbrush => {
            for (x, y) in [(0.2, 0.3), (0.5, 0.2), (0.8, 0.35), (0.35, 0.55), (0.65, 0.6), (0.5, 0.85), (0.15, 0.75), (0.85, 0.8)] {
                p.circle_filled(at(x, y), 1.4, c);
            }
        }
        Tool::Arrow => {
            p.line_segment([at(0.05, 0.95), at(0.9, 0.1)], st);
            p.line_segment([at(0.9, 0.1), at(0.55, 0.12)], st);
            p.line_segment([at(0.9, 0.1), at(0.88, 0.45)], st);
        }
        Tool::Eraser => {
            p.add(egui::Shape::convex_polygon(vec![at(0.1, 0.7), at(0.5, 0.15), at(0.95, 0.4), at(0.55, 0.95)], Color32::from_rgb(240, 160, 170), st));
        }
        Tool::Freehand => {
            let pts: Vec<Pos2> = (0..=10).map(|i| at(i as f32 / 10.0, 0.5 + 0.35 * (i as f32 * 0.9).sin())).collect();
            p.add(egui::Shape::line(pts, st));
        }
        Tool::Line => {
            p.line_segment([at(0.05, 0.95), at(0.95, 0.05)], st);
        }
        Tool::Rect => {
            p.rect_stroke(Rect::from_min_max(at(0.05, 0.2), at(0.95, 0.8)), 0.0, st, egui::StrokeKind::Middle);
        }
        Tool::RoundRect => {
            p.rect_stroke(Rect::from_min_max(at(0.05, 0.2), at(0.95, 0.8)), 4.0, st, egui::StrokeKind::Middle);
        }
        Tool::Ellipse => {
            p.add(EllipseShape::stroke(r.center(), Vec2::new(r.width() * 0.45, r.height() * 0.32), st));
        }
        Tool::FilledRect => {
            p.rect_filled(Rect::from_min_max(at(0.05, 0.2), at(0.95, 0.8)), 0.0, c);
        }
        Tool::FilledRoundRect => {
            p.rect_filled(Rect::from_min_max(at(0.05, 0.2), at(0.95, 0.8)), 4.0, c);
        }
        Tool::FilledEllipse => {
            p.add(EllipseShape::filled(r.center(), Vec2::new(r.width() * 0.45, r.height() * 0.32), c));
        }
        Tool::Magnify => {
            p.circle_stroke(at(0.4, 0.4), r.width() * 0.32, st);
            p.line_segment([at(0.63, 0.63), at(0.95, 0.95)], Stroke::new(2.5, c));
        }
        Tool::ViewOnly => {
            // open hand: palm and four fingers
            p.rect_stroke(Rect::from_min_max(at(0.2, 0.5), at(0.8, 0.95)), 3.0, st, egui::StrokeKind::Middle);
            for i in 0..4 {
                let x = 0.25 + i as f32 * 0.17;
                p.line_segment([at(x, 0.5), at(x, 0.1 + (i % 2) as f32 * 0.08)], st);
            }
        }
    }
    let _ = (rt, b);
}
