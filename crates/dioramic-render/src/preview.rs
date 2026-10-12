//! Downscaled preview for the AI and thumbnails.
//!
//! Area averaging (box filter with fractional coverage) on premultiplied pixels: every source pixel
//! contributes exactly its covered area, so thin lines and text stay smooth instead of aliasing the
//! way point-sampled bilinear/bicubic do at 2–4× reductions.

use resvg::tiny_skia::{IntSize, Pixmap};

use crate::error::RenderError;

const CHANNELS: usize = 4;

/// Downscales (high quality filter) so the longest side is ≤ `max_side`. Smaller images are returned as is.
pub fn downscale(pixmap: &Pixmap, max_side: u32) -> Result<Pixmap, RenderError> {
    if max_side == 0 {
        return Err(RenderError::Request("preview max side must be > 0".to_owned()));
    }
    let longest = pixmap.width().max(pixmap.height());
    if longest <= max_side {
        return Ok(pixmap.clone());
    }
    let factor = f64::from(max_side) / f64::from(longest);
    let fit = |side: u32| ((f64::from(side) * factor).round() as u32).clamp(1, max_side);
    resize_area(pixmap, fit(pixmap.width()), fit(pixmap.height()))
}

/// Area-average resize to exactly `width × height` (meant for reductions).
pub(crate) fn resize_area(pixmap: &Pixmap, width: u32, height: u32) -> Result<Pixmap, RenderError> {
    let (source_width, source_height) = (pixmap.width() as usize, pixmap.height() as usize);
    let columns = coverage_weights(source_width, width as usize);
    let rows = coverage_weights(source_height, height as usize);
    let source = pixmap.data();

    let mut horizontal = vec![0.0_f32; width as usize * source_height * CHANNELS];
    for y in 0..source_height {
        for (x, taps) in columns.iter().enumerate() {
            let out = (y * width as usize + x) * CHANNELS;
            for &(sx, weight) in taps {
                let input = (y * source_width + sx) * CHANNELS;
                for channel in 0..CHANNELS {
                    horizontal[out + channel] += f32::from(source[input + channel]) * weight;
                }
            }
        }
    }

    let mut data = vec![0_u8; width as usize * height as usize * CHANNELS];
    for (y, taps) in rows.iter().enumerate() {
        for x in 0..width as usize {
            let mut sum = [0.0_f32; CHANNELS];
            for &(sy, weight) in taps {
                let input = (sy * width as usize + x) * CHANNELS;
                for (channel, total) in sum.iter_mut().enumerate() {
                    *total += horizontal[input + channel] * weight;
                }
            }
            let out = (y * width as usize + x) * CHANNELS;
            for (channel, total) in sum.iter().enumerate() {
                data[out + channel] = total.round().clamp(0.0, 255.0) as u8;
            }
        }
    }

    let size = IntSize::from_wh(width, height)
        .ok_or_else(|| RenderError::Raster(format!("invalid preview size {width}×{height}")))?;
    Pixmap::from_vec(data, size).ok_or_else(|| RenderError::Raster("preview buffer has the wrong length".to_owned()))
}

/// For each output index, the source indices it covers and their normalized weights.
fn coverage_weights(source: usize, target: usize) -> Vec<Vec<(usize, f32)>> {
    let step = source as f64 / target as f64;
    (0..target)
        .map(|index| {
            let (start, end) = (index as f64 * step, (index + 1) as f64 * step);
            let first = start.floor() as usize;
            let last = (end.ceil() as usize).min(source);
            (first..last)
                .map(|sample| {
                    let covered = end.min(sample as f64 + 1.0) - start.max(sample as f64);
                    (sample, (covered / step) as f32)
                })
                .filter(|&(_, weight)| weight > 0.0)
                .collect()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::{Color, Paint, Rect, Transform};

    use super::*;

    #[test]
    fn longest_side_fits_and_aspect_is_kept() {
        let pixmap = Pixmap::new(3200, 2250).unwrap();
        let preview = downscale(&pixmap, 1024).unwrap();
        assert_eq!((preview.width(), preview.height()), (1024, 720));
    }

    #[test]
    fn small_images_are_untouched() {
        let pixmap = Pixmap::new(300, 200).unwrap();
        assert_eq!(downscale(&pixmap, 1024).unwrap(), pixmap);
    }

    #[test]
    fn halving_a_checkerboard_averages_to_grey() {
        let mut pixmap = Pixmap::new(64, 64).unwrap();
        pixmap.fill(Color::WHITE);
        let mut paint = Paint::default();
        paint.set_color(Color::BLACK);
        for y in 0..64 {
            for x in 0..64 {
                if (x + y) % 2 == 0 {
                    let cell = Rect::from_xywh(x as f32, y as f32, 1.0, 1.0).unwrap();
                    pixmap.fill_rect(cell, &paint, Transform::identity(), None);
                }
            }
        }
        let preview = downscale(&pixmap, 32).unwrap();
        // A point sampler would return pure black or white; area averaging gives mid grey everywhere.
        assert!(preview.pixels().iter().all(|pixel| (120..=135).contains(&pixel.red())), "aliasing");
    }

    #[test]
    fn weights_sum_to_one() {
        for (source, target) in [(10, 3), (1600, 1024), (7, 7)] {
            for taps in coverage_weights(source, target) {
                let total: f32 = taps.iter().map(|(_, weight)| weight).sum();
                assert!((total - 1.0).abs() < 1e-4, "{source}->{target}: {total}");
            }
        }
    }

    #[test]
    fn zero_max_side_is_rejected() {
        assert!(matches!(downscale(&Pixmap::new(2, 2).unwrap(), 0), Err(RenderError::Request(_))));
    }
}
