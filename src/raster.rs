//! A small CPU renderer for egui's tessellated output, drawing into a
//! premultiplied ARGB buffer (wl_shm's `Argb8888`). A bar is a thin strip,
//! so this is cheap, and it needs no GPU, no EGL and no GL driver, which a
//! VM or a software-rendered compositor may not have (ADR-0002).
//!
//! Like egui's own GL and wgpu painters, it blends in gamma space with
//! premultiplied alpha, and leaves anti-aliasing to egui's feathering.

use std::collections::HashMap;

use egui::epaint::{ClippedPrimitive, Primitive, Vertex};
use egui::{Color32, TextureFilter, TextureId, TexturesDelta};

struct Texture {
    width: usize,
    height: usize,
    pixels: Vec<Color32>,
    nearest: bool,
}

impl Texture {
    fn texel(&self, x: isize, y: isize) -> Color32 {
        let x = x.clamp(0, self.width as isize - 1) as usize;
        let y = y.clamp(0, self.height as isize - 1) as usize;
        self.pixels[y * self.width + x]
    }

    fn sample(&self, u: f32, v: f32) -> Color32 {
        let x = u * self.width as f32 - 0.5;
        let y = v * self.height as f32 - 0.5;
        if self.nearest {
            return self.texel(x.round() as isize, y.round() as isize);
        }
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let [a, b, c, d] = [
            self.texel(x0, y0),
            self.texel(x0 + 1, y0),
            self.texel(x0, y0 + 1),
            self.texel(x0 + 1, y0 + 1),
        ];
        let mix = |i: usize| {
            let top = a[i] as f32 * (1.0 - fx) + b[i] as f32 * fx;
            let bottom = c[i] as f32 * (1.0 - fx) + d[i] as f32 * fx;
            (top * (1.0 - fy) + bottom * fy + 0.5) as u8
        };
        Color32::from_rgba_premultiplied(mix(0), mix(1), mix(2), mix(3))
    }
}

/// egui's textures, kept between frames.
#[derive(Default)]
pub struct Painter {
    textures: HashMap<TextureId, Texture>,
}

impl Painter {
    /// Clears `target` to transparent and paints `primitives` into it.
    /// `target` is `width * height` premultiplied ARGB pixels.
    pub fn paint(
        &mut self,
        target: &mut [u32],
        width: usize,
        height: usize,
        pixels_per_point: f32,
        primitives: &[ClippedPrimitive],
        textures: &TexturesDelta,
    ) {
        assert_eq!(target.len(), width * height);
        for (id, delta) in &textures.set {
            self.set_texture(*id, delta);
        }
        target.fill(0);
        for p in primitives {
            let Primitive::Mesh(mesh) = &p.primitive else {
                continue;
            };
            let Some(texture) = self.textures.get(&mesh.texture_id) else {
                continue;
            };
            let clip = p.clip_rect * pixels_per_point;
            let clip = [
                clip.min.x.floor().max(0.0) as i32,
                clip.min.y.floor().max(0.0) as i32,
                (clip.max.x.ceil() as i32).min(width as i32),
                (clip.max.y.ceil() as i32).min(height as i32),
            ];
            if clip[0] >= clip[2] || clip[1] >= clip[3] {
                continue;
            }
            for tri in mesh.indices.chunks_exact(3) {
                let v = [tri[0], tri[1], tri[2]].map(|i| &mesh.vertices[i as usize]);
                triangle(target, width, pixels_per_point, clip, v, texture);
            }
        }
        for id in &textures.free {
            self.textures.remove(id);
        }
    }

    fn set_texture(&mut self, id: TextureId, delta: &egui::epaint::ImageDelta) {
        let egui::ImageData::Color(image) = &delta.image;
        let [w, h] = image.size;
        let nearest = delta.options.magnification == TextureFilter::Nearest;
        match delta.pos {
            None => {
                self.textures.insert(
                    id,
                    Texture {
                        width: w,
                        height: h,
                        pixels: image.pixels.clone(),
                        nearest,
                    },
                );
            }
            Some([x, y]) => {
                let Some(t) = self.textures.get_mut(&id) else {
                    return;
                };
                for row in 0..h {
                    let dst = (y + row) * t.width + x;
                    t.pixels[dst..dst + w].copy_from_slice(&image.pixels[row * w..(row + 1) * w]);
                }
            }
        }
    }
}

/// Fills one triangle, sampling at pixel centres. Edges follow the top-left
/// rule, so triangles that share an edge never paint its pixels twice.
fn triangle(
    target: &mut [u32],
    width: usize,
    ppp: f32,
    clip: [i32; 4],
    v: [&Vertex; 3],
    texture: &Texture,
) {
    let p = v.map(|v| (v.pos.x * ppp, v.pos.y * ppp));
    let mut area = edge(p[0], p[1], p[2]);
    if area.abs() < f32::EPSILON {
        return;
    }
    // Wind every triangle the same way, so "inside" is always positive.
    let (mut v, mut p) = (v, p);
    if area < 0.0 {
        v.swap(1, 2);
        p.swap(1, 2);
        area = -area;
    }
    let min_x = (p[0].0.min(p[1].0).min(p[2].0).floor() as i32).max(clip[0]);
    let min_y = (p[0].1.min(p[1].1).min(p[2].1).floor() as i32).max(clip[1]);
    let max_x = (p[0].0.max(p[1].0).max(p[2].0).ceil() as i32).min(clip[2]);
    let max_y = (p[0].1.max(p[1].1).max(p[2].1).ceil() as i32).min(clip[3]);
    // Edge i is the one opposite vertex i.
    let edges = [(p[1], p[2]), (p[2], p[0]), (p[0], p[1])];
    let top_left = edges.map(|(a, b)| {
        let (dx, dy) = (b.0 - a.0, b.1 - a.1);
        (dy == 0.0 && dx < 0.0) || dy < 0.0
    });
    let solid = v.iter().all(|w| w.uv == v[0].uv);
    let solid_texel = texture.sample(v[0].uv.x, v[0].uv.y);

    for y in min_y..max_y {
        let py = y as f32 + 0.5;
        for x in min_x..max_x {
            let px = x as f32 + 0.5;
            let mut w = [0.0f32; 3];
            let mut inside = true;
            for i in 0..3 {
                w[i] = edge(edges[i].0, edges[i].1, (px, py));
                if w[i] < 0.0 || (w[i] == 0.0 && !top_left[i]) {
                    inside = false;
                    break;
                }
            }
            if !inside {
                continue;
            }
            let w = w.map(|w| w / area);
            let lerp =
                |f: &dyn Fn(&Vertex) -> f32| w[0] * f(v[0]) + w[1] * f(v[1]) + w[2] * f(v[2]);
            let texel = if solid {
                solid_texel
            } else {
                texture.sample(lerp(&|v| v.uv.x), lerp(&|v| v.uv.y))
            };
            let channel = |i: usize| lerp(&|v| v.color[i] as f32) * texel[i] as f32 / 255.0;
            let src = [channel(0), channel(1), channel(2), channel(3)];
            if src[3] <= 0.0 && src[0] <= 0.0 && src[1] <= 0.0 && src[2] <= 0.0 {
                continue;
            }
            let i = y as usize * width + x as usize;
            target[i] = blend(target[i], src);
        }
    }
}

fn edge(a: (f32, f32), b: (f32, f32), p: (f32, f32)) -> f32 {
    (b.0 - a.0) * (p.1 - a.1) - (b.1 - a.1) * (p.0 - a.0)
}

/// Premultiplied "over": src + dst * (1 - src.a). `src` is RGBA 0..=255.
fn blend(dst: u32, src: [f32; 4]) -> u32 {
    let keep = 1.0 - src[3] / 255.0;
    let d = |shift: u32| ((dst >> shift) & 0xff) as f32;
    let c = |s: f32, d: f32| (s + d * keep + 0.5).clamp(0.0, 255.0) as u32;
    let (r, g, b, a) = (
        c(src[0], d(16)),
        c(src[1], d(8)),
        c(src[2], d(0)),
        c(src[3], d(24)),
    );
    (a << 24) | (r << 16) | (g << 8) | b
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui::{Pos2, Rect, Stroke, epaint::Shape};

    /// Runs `draw` through egui and paints the result into a `w` x `h` buffer.
    fn render(w: usize, h: usize, ppp: f32, draw: impl Fn(&egui::Painter)) -> Vec<u32> {
        let ctx = egui::Context::default();
        let size = egui::vec2(w as f32, h as f32) / ppp;
        let mut input = egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(ppp);
        let out = ctx.run_ui(input, |ui| draw(ui.painter()));
        let prims = ctx.tessellate(out.shapes, out.pixels_per_point);
        let mut target = vec![0; w * h];
        Painter::default().paint(
            &mut target,
            w,
            h,
            out.pixels_per_point,
            &prims,
            &out.textures_delta,
        );
        target
    }

    #[test]
    fn fills_a_rect_exactly() {
        let px = render(8, 4, 1.0, |p| {
            p.add(Shape::rect_filled(
                Rect::from_min_max(Pos2::new(2.0, 1.0), Pos2::new(6.0, 3.0)),
                0.0,
                Color32::RED,
            ));
        });
        for y in 0..4 {
            for x in 0..8 {
                let inside = (2..6).contains(&x) && (1..3).contains(&y);
                let want = if inside { 0xffff0000 } else { 0 };
                assert_eq!(px[y * 8 + x], want, "pixel {x},{y}");
            }
        }
    }

    #[test]
    fn shared_edges_are_painted_once() {
        // A translucent rect is two triangles; a pixel painted twice would
        // come out more opaque than the rest.
        let px = render(16, 16, 1.0, |p| {
            p.add(Shape::rect_filled(
                Rect::from_min_max(Pos2::ZERO, Pos2::new(16.0, 16.0)),
                0.0,
                Color32::from_black_alpha(128),
            ));
        });
        assert!(
            px.iter().all(|&p| p == px[0]),
            "uneven alpha: {:x?}",
            px.iter().collect::<std::collections::BTreeSet<_>>()
        );
        assert_eq!(px[0] >> 24, 128);
    }

    #[test]
    fn scales_to_pixels() {
        let px = render(8, 8, 2.0, |p| {
            p.add(Shape::rect_filled(
                Rect::from_min_max(Pos2::ZERO, Pos2::new(2.0, 2.0)),
                0.0,
                Color32::WHITE,
            ));
        });
        assert_eq!(px[3 * 8 + 3], 0xffffffff);
        assert_eq!(px[4 * 8 + 4], 0);
    }

    #[test]
    fn draws_text() {
        let px = render(64, 20, 1.0, |p| {
            p.text(
                Pos2::new(2.0, 2.0),
                egui::Align2::LEFT_TOP,
                "Hi",
                egui::FontId::proportional(14.0),
                Color32::WHITE,
            );
        });
        let lit = px.iter().filter(|&&p| p >> 24 > 0).count();
        assert!(lit > 20, "only {lit} pixels of text");
        assert!(
            px.iter().all(|&p| p >> 24 == 0 || (p & 0xff) <= p >> 24),
            "not premultiplied"
        );
    }

    #[test]
    fn clips() {
        let px = render(8, 8, 1.0, |p| {
            let p = p.with_clip_rect(Rect::from_min_max(Pos2::ZERO, Pos2::new(4.0, 8.0)));
            p.add(Shape::rect_filled(
                Rect::from_min_max(Pos2::ZERO, Pos2::new(8.0, 8.0)),
                0.0,
                Color32::RED,
            ));
            p.add(Shape::line_segment(
                [Pos2::new(0.0, 4.0), Pos2::new(8.0, 4.0)],
                Stroke::new(1.0, Color32::BLUE),
            ));
        });
        assert!((0..8).all(|y| px[y * 8 + 5] == 0));
        assert_eq!(px[0] >> 24, 0xff);
    }

    #[test]
    fn blends_over() {
        assert_eq!(blend(0xff0000ff, [128.0, 0.0, 0.0, 128.0]), 0xff80007f);
    }
}
