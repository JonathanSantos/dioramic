//! Small pixel operations on premultiplied RGBA pixmaps: area-average downscale and zoomed crops.

use dioramic_render::{MAX_SIDE_PX, Pixmap};

const CHANNELS: usize = 4;

/// A region of an image in pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Region {
    /// Left.
    pub x: u32,
    /// Top.
    pub y: u32,
    /// Width.
    pub width: u32,
    /// Height.
    pub height: u32,
}

impl Region {
    /// `x<x>-y<y>-w<w>-h<h>` for file names.
    pub fn label(&self) -> String {
        format!("x{}-y{}-w{}-h{}", self.x, self.y, self.width, self.height)
    }

    /// The part of the region inside a `width × height` image, if any.
    fn clamped(&self, width: u32, height: u32) -> Option<Region> {
        let visible_width = self.width.min(width.saturating_sub(self.x));
        let visible_height = self.height.min(height.saturating_sub(self.y));
        (visible_width > 0 && visible_height > 0).then_some(Region {
            width: visible_width,
            height: visible_height,
            ..*self
        })
    }
}

/// Downscales (area average, so thin strokes do not alias away) to fit `max_width × max_height`.
/// Never upscales.
pub fn fit_within(source: &Pixmap, max_width: u32, max_height: u32) -> Option<Pixmap> {
    let (width, height) = (source.width(), source.height());
    let scale = (f64::from(max_width) / f64::from(width)).min(f64::from(max_height) / f64::from(height)).min(1.0);
    let target_width = ((f64::from(width) * scale).round() as u32).max(1);
    let target_height = ((f64::from(height) * scale).round() as u32).max(1);
    let mut target = Pixmap::new(target_width, target_height)?;
    let source_data = source.data();
    let target_data = target.data_mut();
    for target_y in 0..target_height {
        let (y0, y1) = source_span(target_y, target_height, height);
        for target_x in 0..target_width {
            let (x0, x1) = source_span(target_x, target_width, width);
            let mut sums = [0_u64; CHANNELS];
            for y in y0..y1 {
                for x in x0..x1 {
                    let index = pixel_index(x, y, width);
                    for (sum, value) in sums.iter_mut().zip(&source_data[index..index + CHANNELS]) {
                        *sum += u64::from(*value);
                    }
                }
            }
            let count = u64::from((y1 - y0) * (x1 - x0));
            let index = pixel_index(target_x, target_y, target_width);
            for (value, sum) in target_data[index..index + CHANNELS].iter_mut().zip(sums) {
                *value = ((sum + count / 2) / count) as u8;
            }
        }
    }
    Some(target)
}

/// Source pixels `[start, end)` covered by target pixel `index` of `target_size`.
fn source_span(index: u32, target_size: u32, source_size: u32) -> (u32, u32) {
    let start = (u64::from(index) * u64::from(source_size) / u64::from(target_size)) as u32;
    let end = (u64::from(index + 1) * u64::from(source_size) / u64::from(target_size)) as u32;
    (start, end.max(start + 1).min(source_size))
}

fn pixel_index(x: u32, y: u32, width: u32) -> usize {
    (y as usize * width as usize + x as usize) * CHANNELS
}

/// Cuts `region` out of `source` and enlarges it `zoom` times with nearest-neighbour (crisp pixels for
/// inspecting anti-aliasing, arrow tips and label edges). Errors when the region misses the image or the
/// output exceeds [`MAX_SIDE_PX`].
pub fn crop_zoom(source: &Pixmap, region: Region, zoom: u32) -> Result<Pixmap, String> {
    let region = region.clamped(source.width(), source.height()).ok_or_else(|| {
        format!("crop {} is outside the {}×{} image", region.label(), source.width(), source.height())
    })?;
    let (width, height) = (region.width * zoom, region.height * zoom);
    if width > MAX_SIDE_PX || height > MAX_SIDE_PX {
        return Err(format!("crop {} × zoom {zoom} = {width}×{height} px exceeds {MAX_SIDE_PX} px", region.label()));
    }
    let mut target = Pixmap::new(width, height).ok_or_else(|| "empty crop".to_owned())?;
    let source_data = source.data();
    let target_data = target.data_mut();
    for target_y in 0..height {
        for target_x in 0..width {
            let from = pixel_index(region.x + target_x / zoom, region.y + target_y / zoom, source.width());
            let to = pixel_index(target_x, target_y, width);
            target_data[to..to + CHANNELS].copy_from_slice(&source_data[from..from + CHANNELS]);
        }
    }
    Ok(target)
}

#[cfg(test)]
mod tests {
    use dioramic_render::Pixmap;

    use super::*;

    fn checker(width: u32, height: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap_or_else(|| panic!("pixmap"));
        for (index, pixel) in pixmap.data_mut().as_chunks_mut::<CHANNELS>().0.iter_mut().enumerate() {
            let x = index as u32 % width;
            let y = index as u32 / width;
            let value = if (x + y).is_multiple_of(2) { 255 } else { 0 };
            *pixel = [value, value, value, 255];
        }
        pixmap
    }

    #[test]
    fn downscale_averages_and_keeps_aspect_ratio() {
        let small = fit_within(&checker(100, 50), 10, 10).unwrap_or_else(|| panic!("fit"));
        assert_eq!((small.width(), small.height()), (10, 5));
        let grey = small.data()[0];
        assert!((120..=135).contains(&grey), "checker should average to mid grey, got {grey}");
    }

    #[test]
    fn small_images_are_not_upscaled() {
        let same = fit_within(&checker(8, 8), 100, 100).unwrap_or_else(|| panic!("fit"));
        assert_eq!((same.width(), same.height()), (8, 8));
    }

    #[test]
    fn crop_zoom_repeats_pixels() {
        let source = checker(10, 10);
        let crop =
            crop_zoom(&source, Region { x: 1, y: 0, width: 2, height: 1 }, 3).unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((crop.width(), crop.height()), (6, 3));
        // Pixel (1, 0) is black, (2, 0) is white.
        assert_eq!(crop.data()[0], 0);
        assert_eq!(crop.data()[3 * CHANNELS], 255);
    }

    #[test]
    fn crop_outside_the_image_is_an_error() {
        let error = crop_zoom(&checker(10, 10), Region { x: 20, y: 0, width: 5, height: 5 }, 1);
        assert!(error.is_err_and(|message| message.contains("outside")));
    }

    #[test]
    fn crop_is_clamped_to_the_image() {
        let crop = crop_zoom(&checker(10, 10), Region { x: 8, y: 8, width: 5, height: 5 }, 2)
            .unwrap_or_else(|error| panic!("{error}"));
        assert_eq!((crop.width(), crop.height()), (4, 4));
    }
}
