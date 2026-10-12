//! Global GIF palette: k-means in Oklab with a reserved slice for soft ramps, and the color → index mapper.

use std::collections::HashMap;

use quantette::color_space::srgb8_to_oklab;
use quantette::deps::palette::{Oklab, Srgb};
use quantette::{PaletteSize, Pipeline, QuantizeMethod};
use resvg::tiny_skia::{Pixmap, PremultipliedColorU8};

use super::dither::{Mix, mix_for};
use crate::error::RenderError;
use crate::frames::evenly_spaced_indices;

/// Every n-th pixel of the sampled frames feeds k-means: same palette quality (flat-color art repeats
/// its colors everywhere) at a fraction of the clustering time. Odd so samples do not lock onto columns.
const PALETTE_PIXEL_STRIDE: usize = 3;
/// Largest per-channel step between neighbors that still counts as a soft ramp (blurred shadow, gradient).
/// Antialiased edges and flat fills fall outside `1..=SOFT_RAMP_MAX_STEP`.
const SOFT_RAMP_MAX_STEP: u8 = 3;
/// Palette entries reserved for soft ramps, as a divisor of the palette size (255 / 4 = 63 entries).
/// Ramps cover little area with many close colors, so area-weighted k-means leaves them a handful of
/// entries and they band into rings (T-321); a reserved budget keeps their steps to a few levels.
const SOFT_RAMP_PALETTE_DIVISOR: u16 = 4;
/// Palette entries closer than this (Oklab, ≈ 3 sRGB levels of gray) are interchangeable on screen:
/// a delta frame does not repaint one with the other.
const INTERCHANGEABLE_DISTANCE: f32 = 0.011;
/// Largest palette leaving room for the transparent index.
pub const MAX_PALETTE_COLORS: u16 = 255;

/// Opaque sRGB of a premultiplied pixel, flattened onto white.
pub(super) fn opaque_rgb(pixel: PremultipliedColorU8) -> Srgb<u8> {
    let uncovered = 255 - pixel.alpha();
    Srgb::new(
        pixel.red().saturating_add(uncovered),
        pixel.green().saturating_add(uncovered),
        pixel.blue().saturating_add(uncovered),
    )
}

fn rgb_key(color: Srgb<u8>) -> u32 {
    (u32::from(color.red) << 16) | (u32::from(color.green) << 8) | u32::from(color.blue)
}

/// Palette of at most `max_colors` entries quantized from `samples` evenly spaced frames: a reserved
/// slice for soft ramps (shadows, gradients) plus the rest from all pixels.
pub(super) fn build_palette(frames: &[Pixmap], samples: usize, max_colors: u16) -> Result<Vec<Srgb<u8>>, RenderError> {
    let colors = max_colors.clamp(1, MAX_PALETTE_COLORS);
    let width = frames.first().map_or(1, |frame| frame.width() as usize);
    let sampled: Vec<Vec<Srgb<u8>>> = evenly_spaced_indices(frames.len(), samples)
        .into_iter()
        .map(|index| frames[index].pixels().iter().map(|pixel| opaque_rgb(*pixel)).collect())
        .collect();
    let ramp_samples: Vec<Srgb<u8>> = sampled.iter().flat_map(|colors| soft_ramp_pixels(colors, width)).collect();
    let general_samples: Vec<Srgb<u8>> =
        sampled.iter().flat_map(|colors| colors.iter().step_by(PALETTE_PIXEL_STRIDE).copied()).collect();
    let ramp_budget = if ramp_samples.is_empty() { 0 } else { colors / SOFT_RAMP_PALETTE_DIVISOR };
    let mut palette = quantize(&ramp_samples, ramp_budget)?;
    let general_budget = colors.saturating_sub(u16::try_from(palette.len()).unwrap_or(colors));
    palette.extend(quantize(&general_samples, general_budget)?);
    Ok(palette)
}

/// k-means palette of at most `colors` entries (empty when there is nothing to quantize).
fn quantize(samples: &[Srgb<u8>], colors: u16) -> Result<Vec<Srgb<u8>>, RenderError> {
    if samples.is_empty() || colors == 0 {
        return Ok(Vec::new());
    }
    let size =
        PaletteSize::try_from(colors).map_err(|_| RenderError::Request(format!("invalid palette size {colors}")))?;
    let palette = Pipeline::new()
        .palette_size(size)
        .quantize_method(QuantizeMethod::kmeans())
        .ditherer(None)
        .parallel(true)
        .input_slice(samples)
        .map_err(|error| RenderError::Encode(format!("GIF palette: {error}")))?
        .output_srgb8_palette()
        .into_vec();
    Ok(palette)
}

/// Pixels that differ softly from their right or lower neighbor (see [`is_soft_step`]).
fn soft_ramp_pixels(colors: &[Srgb<u8>], width: usize) -> impl Iterator<Item = Srgb<u8>> + '_ {
    colors.iter().enumerate().filter_map(move |(at, color)| {
        let right = (at % width + 1 < width).then(|| colors[at + 1]);
        let below = colors.get(at + width).copied();
        [right, below].into_iter().flatten().any(|neighbor| is_soft_step(*color, neighbor)).then_some(*color)
    })
}

/// Neighbors that differ, but by at most [`SOFT_RAMP_MAX_STEP`] in every channel.
fn is_soft_step(color: Srgb<u8>, neighbor: Srgb<u8>) -> bool {
    let steps =
        [color.red.abs_diff(neighbor.red), color.green.abs_diff(neighbor.green), color.blue.abs_diff(neighbor.blue)];
    let largest = steps.into_iter().max().unwrap_or(0);
    (1..=SOFT_RAMP_MAX_STEP).contains(&largest)
}

/// Maps colors to palette indices in Oklab (the space the palette was built in).
pub(super) struct PaletteMapper {
    srgb: Vec<Srgb<u8>>,
    oklab: Vec<Oklab>,
    /// `interchangeable[a * len + b]`: entries `a` and `b` look the same (see [`INTERCHANGEABLE_DISTANCE`]).
    interchangeable: Vec<bool>,
}

impl PaletteMapper {
    pub(super) fn new(palette: &[Srgb<u8>]) -> Self {
        let oklab = srgb8_to_oklab(palette);
        let interchangeable = oklab
            .iter()
            .flat_map(|left| oklab.iter().map(move |right| oklab_distance(*left, *right) <= INTERCHANGEABLE_DISTANCE))
            .collect();
        Self { srgb: palette.to_vec(), oklab, interchangeable }
    }

    /// Index reserved for "unchanged" pixels: right after the colors.
    pub(super) fn transparent_index(&self) -> u8 {
        u8::try_from(self.srgb.len()).unwrap_or(u8::MAX)
    }

    /// Whether showing entry `shown` where `wanted` is due is visually lossless.
    pub(super) fn interchangeable(&self, shown: u8, wanted: u8) -> bool {
        let at = usize::from(shown) * self.srgb.len() + usize::from(wanted);
        self.interchangeable.get(at).copied().unwrap_or(false)
    }

    /// Palette mapping of `color`: nearest entry, dithered with a close neighbor on soft ramps.
    fn mix(&self, color: Srgb<u8>) -> Mix {
        let target = srgb8_to_oklab(&[color]).first().copied().unwrap_or_default();
        mix_for(target, &self.oklab)
    }

    /// `[r, g, b, …]` with the transparent slot appended (its color is never shown).
    pub(super) fn global_palette_bytes(&self) -> Vec<u8> {
        self.srgb.iter().flat_map(|color| [color.red, color.green, color.blue]).chain([0, 0, 0]).collect()
    }
}

fn oklab_distance(left: Oklab, right: Oklab) -> f32 {
    let (dl, da, db) = (left.l - right.l, left.a - right.a, left.b - right.b);
    (dl * dl + da * da + db * db).sqrt()
}

/// Memo of `rgb → palette mix` for one stripe of a frame (antialiased art has few distinct colors).
pub(super) struct IndexCache<'a> {
    mapper: &'a PaletteMapper,
    memo: HashMap<u32, Mix>,
}

impl<'a> IndexCache<'a> {
    pub(super) fn new(mapper: &'a PaletteMapper) -> Self {
        Self { mapper, memo: HashMap::new() }
    }

    /// Palette index of `pixel` at `(x, y)` (the dither pattern depends on the position).
    pub(super) fn index(&mut self, pixel: PremultipliedColorU8, x: usize, y: usize) -> u8 {
        let color = opaque_rgb(pixel);
        let mix = *self.memo.entry(rgb_key(color)).or_insert_with(|| self.mapper.mix(color));
        mix.index_at(x, y)
    }
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::Color;

    use super::*;

    fn gray_ramp(width: u32, height: u32) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap();
        for (at, pixel) in pixmap.pixels_mut().iter_mut().enumerate() {
            let level = 120 + (at % width as usize) as u8 / 2;
            *pixel = PremultipliedColorU8::from_rgba(level, level, level, 255).unwrap();
        }
        pixmap
    }

    #[test]
    fn translucent_pixels_flatten_onto_white() {
        let transparent = PremultipliedColorU8::from_rgba(0, 0, 0, 0).unwrap();
        assert_eq!(opaque_rgb(transparent), Srgb::new(255, 255, 255));
    }

    #[test]
    fn soft_step_excludes_flat_and_sharp_neighbors() {
        let gray = |level| Srgb::new(level, level, level);
        assert!(!is_soft_step(gray(100), gray(100)));
        assert!(is_soft_step(gray(100), gray(103)));
        assert!(!is_soft_step(gray(100), gray(140)));
    }

    #[test]
    fn soft_ramp_gets_a_reserved_slice_of_the_palette() {
        let mut frame = gray_ramp(64, 8);
        frame.fill_rect(
            resvg::tiny_skia::Rect::from_xywh(0.0, 0.0, 8.0, 8.0).unwrap(),
            &resvg::tiny_skia::Paint {
                shader: resvg::tiny_skia::Shader::SolidColor(Color::BLACK),
                ..Default::default()
            },
            resvg::tiny_skia::Transform::identity(),
            None,
        );
        let palette = build_palette(&[frame], 1, 16).unwrap();
        assert!(palette.len() <= 16);
        let ramp_entries = palette.iter().filter(|color| (120..=152).contains(&color.red)).count();
        assert!(ramp_entries >= 4, "{palette:?}");
    }

    #[test]
    fn close_entries_are_interchangeable_and_distant_ones_are_not() {
        let mapper = PaletteMapper::new(&[Srgb::new(100, 100, 100), Srgb::new(101, 101, 101), Srgb::new(200, 0, 0)]);
        assert!(mapper.interchangeable(0, 1));
        assert!(!mapper.interchangeable(0, 2));
        assert!(mapper.interchangeable(2, 2));
    }
}
