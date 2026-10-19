//! The app icon (assets/icon.svg), drawn at the sizes the tray and the window ask for.

const SVG: &[u8] = include_bytes!("../assets/icon.svg");

/// The icon as `size`×`size` RGBA with straight alpha, row by row.
pub fn rgba(size: u32) -> Option<Vec<u8>> {
    use resvg::{tiny_skia, usvg};
    let tree = usvg::Tree::from_data(SVG, &usvg::Options::default()).ok()?;
    let mut pixmap = tiny_skia::Pixmap::new(size, size)?;
    let scale = size as f32 / tree.size().width().max(tree.size().height());
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    Some(
        pixmap
            .pixels()
            .iter()
            .flat_map(|p| {
                let c = p.demultiply();
                [c.red(), c.green(), c.blue(), c.alpha()]
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    #[test]
    fn the_icon_draws_with_a_clear_corner_and_a_solid_middle() {
        for size in [16, 22, 48, 256] {
            let px = super::rgba(size).unwrap();
            assert_eq!(px.len(), (size * size * 4) as usize);
            let alpha = |x: u32, y: u32| px[((y * size + x) * 4 + 3) as usize];
            assert_eq!(alpha(size / 2, size / 2), 255);
            assert_eq!(alpha(0, 0), 0);
        }
    }
}
