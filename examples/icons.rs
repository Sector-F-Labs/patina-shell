//! Draws the shell's icons at bar size with the bar's own painter and
//! saves them as a PNG, for checking them by eye:
//! `cargo run -p patina-shell --example icons -- out.png`.

use egui::{Color32, Pos2, Rect, vec2};
use patina_shell::{icons, raster::Painter};

fn main() {
    let out = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "icons.png".into());
    let (w, h, ppp) = (520usize, 62usize, 1.5f32);
    let ctx = egui::Context::default();
    let mut input = egui::RawInput {
        screen_rect: Some(Rect::from_min_size(
            Pos2::ZERO,
            vec2(w as f32 / ppp, h as f32 / ppp),
        )),
        ..Default::default()
    };
    input
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(ppp);
    let ink = Color32::from_gray(220);
    let bg = Color32::from_gray(27);
    let red = Color32::from_rgb(255, 80, 80);
    let out_frame = ctx.run_ui(input, |ui| {
        let p = ui.painter();
        p.rect_filled(ui.max_rect(), 0.0, bg);
        let cell = |i: f32| Rect::from_min_size(egui::pos2(4.0 + i * 34.0, 10.0), vec2(28.0, 22.0));
        icons::wifi(p, cell(0.0), Some(80), ink);
        icons::wifi(p, cell(1.0), Some(40), ink);
        icons::wifi(p, cell(2.0), None, ink);
        icons::speaker(p, cell(3.0), 0.8, false, ink);
        icons::speaker(p, cell(4.0), 0.5, true, ink);
        icons::battery(p, cell(5.0), 100, false, ink, bg);
        icons::battery(p, cell(6.0), 65, false, ink, bg);
        icons::battery(p, cell(7.0), 40, true, ink, bg);
        icons::battery(p, cell(8.0), 8, false, red, bg);
    });
    let primitives = ctx.tessellate(out_frame.shapes, out_frame.pixels_per_point);
    let mut pixels = vec![0u32; w * h];
    Painter::default().paint(
        &mut pixels,
        w,
        h,
        ppp,
        &primitives,
        &out_frame.textures_delta,
    );
    let rgba: Vec<u8> = pixels
        .iter()
        .flat_map(|p| [(p >> 16) as u8, (p >> 8) as u8, *p as u8, (p >> 24) as u8])
        .collect();
    image::RgbaImage::from_raw(w as u32, h as u32, rgba)
        .expect("size matches")
        .save(&out)
        .expect("png saved");
    println!("{out}");
}
