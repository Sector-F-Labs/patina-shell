//! The Sector F flask, which the bar's launcher button and the default
//! wallpaper show, and Ubuntu Mono, the wallpaper's lettering. The logo is
//! rasterised as a white mask, to be tinted from the skin, so skins reach
//! it like everything else.

use egui::ColorImage;

const LOGO: &str = include_str!("../assets/sector-f.svg");

/// Ubuntu Mono Regular, under the Ubuntu Font Licence
/// (assets/UbuntuMono-LICENCE.txt).
pub const UBUNTU_MONO: &[u8] = include_bytes!("../assets/UbuntuMono-Regular.ttf");

/// The logo as a white mask `pixels` square: only the alpha comes from the
/// SVG.
pub fn rasterise(pixels: u32) -> Option<ColorImage> {
    let tree = resvg::usvg::Tree::from_str(LOGO, &resvg::usvg::Options::default()).ok()?;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(pixels, pixels)?;
    let scale = pixels as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    let rgba: Vec<u8> = pixmap
        .data()
        .chunks_exact(4)
        .flat_map(|p| [p[3], p[3], p[3], p[3]])
        .collect();
    Some(ColorImage::from_rgba_premultiplied(
        [pixels as usize, pixels as usize],
        &rgba,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_logo_renders_as_a_mask() {
        let image = rasterise(64).expect("logo renders");
        assert_eq!(image.size, [64, 64]);
        let alpha: Vec<u8> = image.pixels.iter().map(|p| p.a()).collect();
        let solid = alpha.iter().filter(|&&a| a == 255).count();
        assert!(solid > 300, "only {solid} solid pixels");
        assert!(alpha.contains(&0), "no background");
        // A white mask, premultiplied: every channel equals alpha.
        assert!(
            image
                .pixels
                .iter()
                .all(|p| p.r() == p.a() && p.g() == p.a() && p.b() == p.a())
        );
    }
}
