//! Small line icons drawn as vectors, in the spirit of SF Symbols: one stroke
//! weight, rounded caps, optical size independent of the display scale.

use egui::{Color32, CornerRadius, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Bubbles,
    Compose,
    Search,
    Gear,
    ArrowUp,
    Ellipsis,
    Plus,
    Check,
    /// Anonymous contact mark: a phone number has no initials to show.
    Person,
    Pin,
    Mute,
    Smile,
    /// Voice call: a diagonal handset with round earpieces.
    Phone,
}

/// Paints `icon` inside `rect`, keeping a 1-point margin so strokes do not clip.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, color: Color32, weight: f32) {
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    let s = Stroke::new(weight, color);
    let line = |a: Pos2, b: Pos2| painter.line_segment([a, b], s);
    match icon {
        Icon::Bubbles => {
            // Two overlapping speech bubbles, wide rather than round so the
            // mark still reads at 24 px.
            let radius = CornerRadius::same((r * 0.42) as u8);
            let back = Rect::from_center_size(
                c + Vec2::new(r * 0.32, -r * 0.30),
                Vec2::new(r * 1.20, r * 0.86),
            );
            let front = Rect::from_center_size(
                c + Vec2::new(-r * 0.22, r * 0.22),
                Vec2::new(r * 1.34, r * 0.94),
            );
            painter.rect_stroke(back, radius, s, egui::StrokeKind::Inside);
            painter.rect_filled(front, radius, color);
        }
        Icon::Compose => {
            let square = Rect::from_center_size(c, Vec2::splat(r * 1.35));
            painter.rect_stroke(
                square,
                CornerRadius::same((r * 0.28) as u8),
                s,
                egui::StrokeKind::Inside,
            );
            line(
                c + Vec2::new(r * 0.15, -r * 0.55),
                c + Vec2::new(r * 0.75, r * 0.05),
            );
        }
        Icon::Search => {
            let lens = c + Vec2::new(-r * 0.18, -r * 0.18);
            painter.circle_stroke(lens, r * 0.55, s);
            line(
                lens + Vec2::splat(r * 0.39),
                c + Vec2::new(r * 0.72, r * 0.72),
            );
        }
        Icon::Gear => {
            // Cog: a ringed wheel with eight teeth, like WhatsApp Web's
            // settings button rather than macOS sliders.
            painter.circle_stroke(c, r * 0.58, s);
            painter.circle_stroke(c, r * 0.22, s);
            for i in 0..8 {
                let angle = i as f32 * std::f32::consts::TAU / 8.0;
                let dir = Vec2::angled(angle);
                line(c + dir * (r * 0.62), c + dir * (r * 0.92));
            }
        }
        Icon::ArrowUp => {
            // Paper plane, the send mark on WhatsApp Web.
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.72, -r * 0.58),
                    c + Vec2::new(r * 0.78, 0.0),
                    c + Vec2::new(-r * 0.72, r * 0.58),
                    c + Vec2::new(-r * 0.34, 0.0),
                    c + Vec2::new(-r * 0.72, -r * 0.58),
                ],
                s,
            ));
            line(c + Vec2::new(-r * 0.34, 0.0), c + Vec2::new(r * 0.78, 0.0));
        }
        Icon::Ellipsis => {
            for dx in [-0.5, 0.0, 0.5] {
                painter.circle_filled(c + Vec2::new(r * dx * 1.1, 0.0), weight * 0.85, color);
            }
        }
        Icon::Plus => {
            line(c + Vec2::new(0.0, -r * 0.7), c + Vec2::new(0.0, r * 0.7));
            line(c + Vec2::new(-r * 0.7, 0.0), c + Vec2::new(r * 0.7, 0.0));
        }
        Icon::Person => {
            painter.circle_filled(c + Vec2::new(0.0, -r * 0.34), r * 0.30, color);
            let shoulders = Rect::from_min_size(
                c + Vec2::new(-r * 0.62, r * 0.12),
                Vec2::new(r * 1.24, r * 0.68),
            );
            painter.rect_filled(
                shoulders,
                CornerRadius {
                    nw: (r * 0.62) as u8,
                    ne: (r * 0.62) as u8,
                    sw: 0,
                    se: 0,
                },
                color,
            );
        }
        Icon::Pin => {
            painter.circle_filled(c + Vec2::new(r * 0.22, -r * 0.22), r * 0.34, color);
            line(
                c + Vec2::new(r * 0.02, r * 0.02),
                c + Vec2::new(-r * 0.55, r * 0.6),
            );
        }
        Icon::Mute => {
            // Speaker with the sound cone and a slash: the muted-chat mark.
            painter.rect_filled(
                Rect::from_min_max(
                    c + Vec2::new(-r * 0.66, -r * 0.20),
                    c + Vec2::new(-r * 0.32, r * 0.20),
                ),
                0.0,
                color,
            );
            painter.add(Shape::convex_polygon(
                vec![
                    c + Vec2::new(-r * 0.32, -r * 0.20),
                    c + Vec2::new(r * 0.10, -r * 0.54),
                    c + Vec2::new(r * 0.10, r * 0.54),
                    c + Vec2::new(-r * 0.32, r * 0.20),
                ],
                color,
                Stroke::NONE,
            ));
            line(
                c + Vec2::new(-r * 0.74, -r * 0.60),
                c + Vec2::new(r * 0.70, r * 0.60),
            );
        }
        Icon::Smile => {
            painter.circle_stroke(c, r * 0.82, s);
            painter.circle_filled(c + Vec2::new(-r * 0.30, -r * 0.18), weight * 0.9, color);
            painter.circle_filled(c + Vec2::new(r * 0.30, -r * 0.18), weight * 0.9, color);
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.36, r * 0.18),
                    c + Vec2::new(0.0, r * 0.44),
                    c + Vec2::new(r * 0.36, r * 0.18),
                ],
                Stroke::new(weight * 0.9, color),
            ));
        }
        Icon::Phone => {
            // Filled handset: two grips joined by a bar, matching the call
            // button on WhatsApp Web.
            painter.add(Shape::convex_polygon(
                vec![
                    c + Vec2::new(-0.78 * r, -0.40 * r),
                    c + Vec2::new(-0.40 * r, -0.78 * r),
                    c + Vec2::new(-0.16 * r, -0.54 * r),
                    c + Vec2::new(-0.54 * r, -0.16 * r),
                ],
                color,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![
                    c + Vec2::new(-0.50 * r, -0.34 * r),
                    c + Vec2::new(-0.34 * r, -0.50 * r),
                    c + Vec2::new(0.50 * r, 0.34 * r),
                    c + Vec2::new(0.34 * r, 0.50 * r),
                ],
                color,
                Stroke::NONE,
            ));
            painter.add(Shape::convex_polygon(
                vec![
                    c + Vec2::new(0.16 * r, 0.54 * r),
                    c + Vec2::new(0.54 * r, 0.16 * r),
                    c + Vec2::new(0.78 * r, 0.40 * r),
                    c + Vec2::new(0.40 * r, 0.78 * r),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Icon::Check => {
            painter.add(Shape::line(
                vec![
                    c + Vec2::new(-r * 0.6, r * 0.0),
                    c + Vec2::new(-r * 0.15, r * 0.45),
                    c + Vec2::new(r * 0.65, -r * 0.45),
                ],
                s,
            ));
        }
    }
}

/// Chromeless circular icon button with an iOS-style hover fill.
pub fn button(ui: &mut Ui, icon: Icon, size: f32, color: Color32, hover: Color32) -> Response {
    let (rect, resp) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    if resp.hovered() {
        ui.painter().circle_filled(rect.center(), size * 0.5, hover);
    }
    paint(
        ui.painter(),
        Rect::from_center_size(rect.center(), Vec2::splat(size * 0.62)),
        icon,
        if resp.hovered() {
            color
        } else {
            color.gamma_multiply(0.9)
        },
        1.6,
    );
    resp
}
