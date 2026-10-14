//! Small icons the shell pieces draw themselves, from shapes in a colour
//! the caller takes from the skin, so they follow it like text does.

use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, pos2, vec2};

/// A speaker in `rect`: with up to three waves for `level` (0 to 1), or a
/// cross when `muted`.
pub fn speaker(painter: &Painter, rect: Rect, level: f32, muted: bool, colour: Color32) {
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    let at = |x: f32, y: f32| pos2(r.left() + x * s, r.top() + y * s);
    // The speaker: a box and its cone.
    painter.add(Shape::convex_polygon(
        vec![
            at(0.10, 0.38),
            at(0.28, 0.38),
            at(0.50, 0.18),
            at(0.50, 0.82),
            at(0.28, 0.62),
            at(0.10, 0.62),
        ],
        colour,
        Stroke::NONE,
    ));
    let stroke = Stroke::new((s * 0.075).max(1.0), colour);
    if muted {
        painter.line_segment([at(0.62, 0.38), at(0.86, 0.62)], stroke);
        painter.line_segment([at(0.62, 0.62), at(0.86, 0.38)], stroke);
        return;
    }
    let waves = if level <= 0.0 {
        0
    } else if level < 0.34 {
        1
    } else if level < 0.67 {
        2
    } else {
        3
    };
    let centre = at(0.50, 0.50);
    for i in 0..waves {
        let radius = s * (0.16 + 0.13 * i as f32);
        painter.add(Shape::line(arc(centre, radius), stroke));
    }
}

/// Points along a quarter-circle arc opening to the right.
fn arc(centre: Pos2, radius: f32) -> Vec<Pos2> {
    (0..=8)
        .map(|i| {
            let a = -std::f32::consts::FRAC_PI_4 + std::f32::consts::FRAC_PI_2 * i as f32 / 8.0;
            centre + vec2(a.cos(), a.sin()) * radius
        })
        .collect()
}
