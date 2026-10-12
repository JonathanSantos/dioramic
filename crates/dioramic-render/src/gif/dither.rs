//! Ramp-only ordered dithering for the GIF palette mapping (T-321).
//!
//! A global 255-color palette spends few entries on soft shadows and radial gradients, so a smooth ramp
//! collapses into concentric bands. For a color that falls *between* two close palette entries we mix them
//! with a 4×4 Bayer pattern in proportion to where the color sits on the segment joining them.
//!
//! Why this shape:
//! - Only pairs closer than [`RAMP_MAX_DISTANCE`] are mixed, so antialiased edges between contrasting colors
//!   (text, outlines) never turn into speckle; flat fills that hit a palette entry stay solid.
//! - The threshold depends only on the pixel position, so a static pixel maps to the same index in every
//!   frame: no shimmer, and delta frames keep skipping it.

use quantette::deps::palette::Oklab;

/// Bayer 4×4 matrix (values 0..16); thresholds are `(value + 0.5) / 16`.
const BAYER_4X4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
const BAYER_LEVELS: f32 = 16.0;
/// Largest Oklab distance between two palette entries that may be mixed (≈ 10 sRGB levels of gray):
/// wide enough for the steps of a shadow ramp, far below the contrast of an antialiased edge.
const RAMP_MAX_DISTANCE: f32 = 0.04;
/// A color farther than this from the mixing segment is not on a ramp between the two entries.
const RAMP_MAX_OFF_AXIS: f32 = 0.006;

/// How a source color maps to the palette: always `nearest`, or a positional mix with `partner`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Mix {
    nearest: u8,
    partner: u8,
    /// Share of pixels that take `partner` (0 = never).
    partner_share: f32,
}

impl Mix {
    /// Palette index for the pixel at `(x, y)`.
    pub(crate) fn index_at(self, x: usize, y: usize) -> u8 {
        if self.partner_share > bayer_threshold(x, y) { self.partner } else { self.nearest }
    }
}

fn bayer_threshold(x: usize, y: usize) -> f32 {
    (f32::from(BAYER_4X4[y % 4][x % 4]) + 0.5) / BAYER_LEVELS
}

/// Mixing rule for `target` against `palette` (in Oklab).
pub(crate) fn mix_for(target: Oklab, palette: &[Oklab]) -> Mix {
    let nearest = nearest_index(target, palette);
    let solid = Mix { nearest: as_index(nearest), partner: as_index(nearest), partner_share: 0.0 };
    let Some(anchor) = palette.get(nearest).copied() else {
        return solid;
    };
    palette
        .iter()
        .enumerate()
        .filter(|&(index, _)| index != nearest)
        .filter_map(|(index, entry)| ramp_position(anchor, *entry, target).map(|ramp| (index, ramp)))
        .min_by(|(_, left), (_, right)| left.partner_distance.total_cmp(&right.partner_distance))
        .map_or(solid, |(partner, ramp)| Mix {
            nearest: as_index(nearest),
            partner: as_index(partner),
            partner_share: ramp.share,
        })
}

/// Where `target` projects on the segment `from → to`.
struct RampPosition {
    /// Fraction of the way toward `to`, in `(0, 1)`.
    share: f32,
    /// Distance from `target` to `to`: the closest qualifying partner gives the finest mix.
    partner_distance: f32,
}

fn ramp_position(from: Oklab, to: Oklab, target: Oklab) -> Option<RampPosition> {
    let axis = [to.l - from.l, to.a - from.a, to.b - from.b];
    let offset = [target.l - from.l, target.a - from.a, target.b - from.b];
    let length_squared = dot(axis, axis);
    if length_squared <= f32::EPSILON || length_squared > RAMP_MAX_DISTANCE * RAMP_MAX_DISTANCE {
        return None;
    }
    let share = dot(offset, axis) / length_squared;
    if share <= 0.0 || share >= 1.0 {
        return None;
    }
    let off_axis = (dot(offset, offset) - share * share * length_squared).max(0.0).sqrt();
    let towards_partner = [to.l - target.l, to.a - target.a, to.b - target.b];
    (off_axis <= RAMP_MAX_OFF_AXIS)
        .then(|| RampPosition { share, partner_distance: dot(towards_partner, towards_partner) })
}

fn dot(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn nearest_index(target: Oklab, palette: &[Oklab]) -> usize {
    let distance = |entry: &Oklab| {
        let delta = [entry.l - target.l, entry.a - target.a, entry.b - target.b];
        dot(delta, delta)
    };
    palette
        .iter()
        .enumerate()
        .min_by(|(_, left), (_, right)| distance(left).total_cmp(&distance(right)))
        .map_or(0, |(index, _)| index)
}

/// Palettes hold at most 255 colors, so every index fits in a byte.
fn as_index(index: usize) -> u8 {
    u8::try_from(index).unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gray(lightness: f32) -> Oklab {
        Oklab::new(lightness, 0.0, 0.0)
    }

    fn partner_share_over_tile(mix: Mix) -> f32 {
        let partners = (0..4).flat_map(|y| (0..4).map(move |x| (x, y))).filter(|&(x, y)| mix.index_at(x, y) == 1);
        partners.count() as f32 / 16.0
    }

    #[test]
    fn exact_palette_color_stays_solid() {
        let palette = [gray(0.5), gray(0.52)];
        let mix = mix_for(gray(0.5), &palette);
        assert_eq!(partner_share_over_tile(mix), 0.0);
    }

    #[test]
    fn color_between_close_entries_mixes_in_proportion() {
        let palette = [gray(0.50), gray(0.52)];
        // 25% of the way from entry 0 to entry 1.
        let mix = mix_for(gray(0.505), &palette);
        assert_eq!(partner_share_over_tile(mix), 0.25);
    }

    #[test]
    fn distant_entries_never_mix() {
        let palette = [gray(0.2), gray(0.8)];
        let mix = mix_for(gray(0.35), &palette);
        assert_eq!(partner_share_over_tile(mix), 0.0);
    }

    #[test]
    fn off_axis_color_is_not_dithered() {
        let palette = [gray(0.50), gray(0.52)];
        let mix = mix_for(Oklab::new(0.51, 0.03, 0.0), &palette);
        assert_eq!(partner_share_over_tile(mix), 0.0);
    }

    #[test]
    fn pattern_depends_only_on_position_modulo_four() {
        let mix = mix_for(gray(0.51), &[gray(0.50), gray(0.52)]);
        assert!((0..4).all(|y| (0..4).all(|x| mix.index_at(x, y) == mix.index_at(x + 8, y + 4))));
    }
}
