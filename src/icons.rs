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

/// A Wi-Fi fan in `rect`: a dot and three arcs, lit up to `strength`
/// (0 to 100), the rest dimmed; with a slash when not connected (`None`).
pub fn wifi(painter: &Painter, rect: Rect, strength: Option<u8>, colour: Color32) {
    let s = rect.height().min(rect.width());
    let r = Rect::from_center_size(rect.center(), vec2(s, s));
    let base = pos2(r.center().x, r.top() + 0.82 * s);
    let dim = colour.gamma_multiply(0.3);
    let lit = match strength {
        None => 0,
        Some(v) if v >= 67 => 3,
        Some(v) if v >= 34 => 2,
        Some(v) if v > 0 => 1,
        Some(_) => 0,
    };
    let stroke_width = (s * 0.09).max(1.0);
    let dot_colour = if strength.is_some() { colour } else { dim };
    painter.circle_filled(base, stroke_width * 0.9, dot_colour);
    for i in 0..3 {
        let radius = s * (0.26 + 0.2 * i as f32);
        let c = if i < lit { colour } else { dim };
        let points: Vec<Pos2> = (0..=10)
            .map(|k| {
                // From up-left to up-right, the fan's opening.
                let a = -std::f32::consts::FRAC_PI_4 * 3.0
                    + std::f32::consts::FRAC_PI_2 * k as f32 / 10.0;
                base + vec2(a.cos(), a.sin()) * radius
            })
            .collect();
        painter.add(Shape::line(points, Stroke::new(stroke_width, c)));
    }
    if strength.is_none() {
        painter.line_segment(
            [
                pos2(r.left() + 0.18 * s, r.top() + 0.18 * s),
                pos2(r.right() - 0.18 * s, r.bottom() - 0.12 * s),
            ],
            Stroke::new(stroke_width, colour),
        );
    }
}
