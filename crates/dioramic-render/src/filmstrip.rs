//! Filmstrip: key frames side by side, because models cannot watch a GIF.

use resvg::tiny_skia::{Color, Pixmap, PixmapPaint, Transform};

use crate::FILMSTRIP_FRAMES;
use crate::error::RenderError;
use crate::frames::evenly_spaced_indices;
use crate::preview::resize_area;

/// Breathing room between two key frames (px in the output).
pub const FILMSTRIP_GAP_PX: u32 = 16;

/// Picks [`crate::FILMSTRIP_FRAMES`] evenly spaced frames and lays them side by side with a thin gap,
/// fitting the result in `max_side` px.
pub fn filmstrip(frames: &[Pixmap], max_side: u32) -> Result<Pixmap, RenderError> {
    let first = frames.first().ok_or_else(|| RenderError::Request("filmstrip needs at least one frame".to_owned()))?;
    let picked = evenly_spaced_indices(frames.len(), FILMSTRIP_FRAMES);
    let count = picked.len() as u32;
    let gaps = FILMSTRIP_GAP_PX * (count - 1);
    if max_side <= gaps {
        return Err(RenderError::Request(format!("filmstrip max side {max_side} is too small")));
    }
    let factor = (f64::from(max_side - gaps) / f64::from(first.width() * count))
        .min(f64::from(max_side) / f64::from(first.height()))
        .min(1.0);
    let cell_width = ((f64::from(first.width()) * factor).floor() as u32).max(1);
    let cell_height = ((f64::from(first.height()) * factor).floor() as u32).max(1);

    let strip_width = cell_width * count + gaps;
    let mut strip = Pixmap::new(strip_width, cell_height)
        .ok_or_else(|| RenderError::Raster(format!("cannot allocate {strip_width}×{cell_height} filmstrip")))?;
    strip.fill(gap_color(first));
    for (slot, &index) in picked.iter().enumerate() {
        let cell = resize_area(&frames[index], cell_width, cell_height)?;
        let x = slot as u32 * (cell_width + FILMSTRIP_GAP_PX);
        strip.draw_pixmap(x as i32, 0, cell.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    Ok(strip)
}

/// The gap takes the page background (top-left pixel) so the strip reads as one image, slightly darkened
/// so the frame boundaries stay visible.
fn gap_color(frame: &Pixmap) -> Color {
    const GAP_SHADE: f32 = 0.88;
    frame.pixel(0, 0).map_or(Color::WHITE, |pixel| {
        let color = pixel.demultiply();
        Color::from_rgba(
            f32::from(color.red()) / 255.0 * GAP_SHADE,
            f32::from(color.green()) / 255.0 * GAP_SHADE,
            f32::from(color.blue()) / 255.0 * GAP_SHADE,
            1.0,
        )
        .unwrap_or(Color::WHITE)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frames(count: usize, width: u32, height: u32) -> Vec<Pixmap> {
        (0..count)
            .map(|index| {
                let mut pixmap = Pixmap::new(width, height).unwrap();
                pixmap.fill(Color::from_rgba8((index * 2) as u8, 0, 0, 255));
                pixmap
            })
            .collect()
    }

    #[test]
    fn four_frames_fit_in_max_side() {
        let strip = filmstrip(&frames(100, 1600, 1125), 1024).unwrap();
        assert!(strip.width() <= 1024 && strip.height() <= 1024);
        let cell = (strip.width() - 3 * FILMSTRIP_GAP_PX) / 4;
        assert_eq!(strip.width(), cell * 4 + 3 * FILMSTRIP_GAP_PX);
        // Second cell is frame 25 (red = 50).
        let red = strip.pixel(cell + FILMSTRIP_GAP_PX + 2, 2).unwrap().red();
        assert_eq!(red, 50);
    }

    #[test]
    fn fewer_frames_than_slots() {
        let strip = filmstrip(&frames(2, 100, 50), 1024).unwrap();
        assert_eq!((strip.width(), strip.height()), (200 + FILMSTRIP_GAP_PX, 50));
    }

    #[test]
    fn empty_input_is_rejected() {
        assert!(matches!(filmstrip(&[], 1024), Err(RenderError::Request(_))));
    }
}
