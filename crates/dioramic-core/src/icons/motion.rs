//! Small looping movements inside the icons (T-402): LEDs blinking in sequence, waves, rings, a turning helm.
//!
//! An icon drawn with `IconDraw.animate` wraps each moving part in a *motion hook*, a `<g>` whose
//! `data-icon-motion` attribute describes the movement ([`IconMotion::attribute`]). Node bodies are drawn
//! once, before the SVG writer knows the output mode, so the writer resolves the hooks per mode
//! (`svg::icon_motion`): stripped in a static SVG (the drawing is byte-identical to an icon drawn without
//! hooks), evaluated at `t` in raster frames, and turned into SMIL in the animated SVG. Both paths read the
//! same piecewise-linear [`Keyframes`], so the GIF and the animated SVG match.
//!
//! Every movement repeats a whole number of times per loop ([`IconMotion::cycles`]), so loops are seamless.

use std::fmt::Write as _;

use crate::canvas::IsoCanvas;
use crate::geometry::{Affine, COS_30, Rect, SIN_30, ScreenPoint, ScreenVector};

/// Attribute that marks a motion hook in node markup.
pub const MOTION_ATTRIBUTE: &str = "data-icon-motion";

/// Opacity an LED dims to when it blinks.
const BLINK_LOW: f64 = 0.3;
/// Peak opacity of an activity light flashing over a port.
const GLINT_PEAK: f64 = 0.95;
/// Lowest opacity of a breathing part.
const BREATHE_LOW: f64 = 0.55;
/// Resting opacity of a Wi-Fi arc between two passing waves.
const WAVE_LOW: f64 = 0.4;
/// Peak opacity of the highlight sweeping over a glyph.
const SHINE_PEAK: f64 = 0.85;
/// A ring starts at this share of its radius and grows to the full radius while it fades out.
const RIPPLE_START_SCALE: f64 = 0.3;
/// Peak opacity of a ring.
const RIPPLE_PEAK: f64 = 0.75;
/// Lowest height of a chart bar, as a share of its drawn height.
const BOB_LOW: f64 = 0.62;
/// Samples per cycle of the smooth (sine-like) movements.
const SMOOTH_SAMPLES: usize = 12;
/// Share of the trip over which a travelling item fades in and out (as the edge packets).
const TRAVEL_FADE: f64 = 0.18;

/// Natural period (s) of each family of movements; the loop runs a whole number of them.
const PERIOD_FAST_S: f64 = 1.0;
const PERIOD_SLOW_S: f64 = 2.0;

/// Piecewise-linear function on one cycle `[0, 1]`: `(time, value)` points with non-decreasing times from 0
/// to 1. Two points at the same time are a jump (the later one wins from that time on).
#[derive(Debug, Clone, PartialEq)]
pub struct Keyframes(Vec<(f64, f64)>);

impl Keyframes {
    /// Keyframes from points (must start at 0 and end at 1).
    pub fn new(points: Vec<(f64, f64)>) -> Self {
        Self(points)
    }

    /// The points.
    pub fn points(&self) -> &[(f64, f64)] {
        &self.0
    }

    /// Value at `u ∈ [0, 1]` (linear between points, right-continuous at jumps).
    pub fn value_at(&self, u: f64) -> f64 {
        let points = &self.0;
        let Some(&(_, first)) = points.first() else { return 0.0 };
        let mut value = first;
        for pair in points.windows(2) {
            let ((t0, v0), (t1, v1)) = (pair[0], pair[1]);
            if u < t0 {
                break;
            }
            value = if u >= t1 {
                v1
            } else if t1 > t0 {
                v0 + (v1 - v0) * (u - t0) / (t1 - t0)
            } else {
                v1
            };
        }
        value
    }

    /// `u ↦ self((u + phase) mod 1)` as keyframes (for SMIL, which has no phase of its own).
    pub fn shifted(&self, phase: f64) -> Self {
        let phase = phase.rem_euclid(1.0);
        if phase == 0.0 {
            return self.clone();
        }
        let start = self.value_at(phase);
        let mut points = vec![(0.0, start)];
        points.extend(self.0.iter().filter(|(time, _)| *time > phase).map(|&(time, value)| (time - phase, value)));
        points
            .extend(self.0.iter().filter(|(time, _)| *time <= phase).map(|&(time, value)| (time + 1.0 - phase, value)));
        points.push((1.0, start));
        points.dedup();
        Self(points)
    }

    fn smooth(low: f64, high: f64) -> Self {
        // Cosine from `high` down to `low` and back, sampled: smooth in both the frames and the SMIL.
        let points = (0..=SMOOTH_SAMPLES)
            .map(|index| {
                let time = index as f64 / SMOOTH_SAMPLES as f64;
                let wave = 0.5 + 0.5 * (std::f64::consts::TAU * time).cos();
                (time, low + (high - low) * wave)
            })
            .collect();
        Self(points)
    }
}

/// A looping movement of one part of an icon.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IconMotion {
    /// An LED that dims briefly once per cycle, at `phase` of it (LEDs blinking in sequence).
    Blink {
        /// Phase in the cycle, `[0, 1)`.
        phase: f64,
    },
    /// An activity light, invisible at rest, flashing once per cycle (ports lighting up).
    Glint {
        /// Phase in the cycle.
        phase: f64,
    },
    /// Opacity breathing smoothly between [`BREATHE_LOW`] and 1.
    Breathe {
        /// Phase in the cycle.
        phase: f64,
    },
    /// A Wi-Fi arc lit by a wave passing outward.
    Wave {
        /// Phase in the cycle.
        phase: f64,
    },
    /// A highlight (invisible at rest) swelling and fading over a glyph.
    Shine {
        /// Phase in the cycle.
        phase: f64,
    },
    /// A ring (invisible at rest) growing from the center of `center` and fading out.
    Ripple {
        /// Screen center of the ring.
        center: ScreenPoint,
        /// Phase in the cycle.
        phase: f64,
    },
    /// A rotation in the floor plane around `center`, by `step_degrees` per cycle (a symmetric glyph's step,
    /// so the turn looks continuous across the loop).
    Spin {
        /// Screen point of the rotation axis on the floor plane.
        center: ScreenPoint,
        /// Plan degrees per cycle.
        step_degrees: f64,
        /// Phase in the cycle.
        phase: f64,
    },
    /// A chart bar growing and shrinking vertically from its base `anchor`.
    Bob {
        /// Screen point at the base of the bar.
        anchor: ScreenPoint,
        /// Phase in the cycle.
        phase: f64,
    },
    /// A sway back and forth by `offset` (screen px) around the rest position.
    Drift {
        /// Largest displacement.
        offset: ScreenVector,
        /// Phase in the cycle.
        phase: f64,
    },
    /// An item (invisible at rest) travelling by `trip` once per cycle, fading in and out at the ends.
    Travel {
        /// Screen displacement over one cycle.
        trip: ScreenVector,
        /// Phase in the cycle.
        phase: f64,
    },
}

/// A transform track of a motion (screen space).
#[derive(Debug, Clone, PartialEq)]
pub enum TransformTrack {
    /// Translation along `direction` by the keyframe value (a factor of `direction`).
    Translate {
        /// Direction (px at value 1).
        direction: ScreenVector,
        /// Factor.
        keys: Keyframes,
    },
    /// Uniform scale around `center`.
    Scale {
        /// Center.
        center: ScreenPoint,
        /// Scale.
        keys: Keyframes,
    },
    /// Vertical scale around `anchor`.
    ScaleY {
        /// Anchor.
        anchor: ScreenPoint,
        /// Vertical scale.
        keys: Keyframes,
    },
    /// Rotation in the floor plane around `center` (degrees).
    FloorRotate {
        /// Screen point of the axis.
        center: ScreenPoint,
        /// Degrees.
        keys: Keyframes,
    },
}

/// Opacity and transform of a moving part at some time.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MotionState {
    /// Opacity (1 = as drawn).
    pub opacity: f64,
    /// Screen transform (identity = as drawn).
    pub transform: Affine,
}

impl IconMotion {
    /// Natural period (s): fast for lights, slow for rings and turns.
    fn period_s(&self) -> f64 {
        match self {
            Self::Blink { .. } | Self::Glint { .. } | Self::Wave { .. } => PERIOD_FAST_S,
            Self::Breathe { .. }
            | Self::Shine { .. }
            | Self::Ripple { .. }
            | Self::Spin { .. }
            | Self::Bob { .. }
            | Self::Drift { .. }
            | Self::Travel { .. } => PERIOD_SLOW_S,
        }
    }

    /// Whole cycles per loop of `duration_s` (at least 1): the movement is seamless at the loop point.
    pub fn cycles(&self, duration_s: f64) -> u32 {
        let cycles = (duration_s / self.period_s()).round();
        if cycles.is_finite() && cycles >= 1.0 { cycles.min(f64::from(u16::MAX)) as u32 } else { 1 }
    }

    /// Parts that only exist while animating (lights over ports, rings, travelling items): a static SVG
    /// drops them, so it looks exactly as without the feature.
    pub fn is_overlay(&self) -> bool {
        matches!(self, Self::Glint { .. } | Self::Shine { .. } | Self::Ripple { .. } | Self::Travel { .. })
    }

    fn phase(&self) -> f64 {
        match *self {
            Self::Blink { phase }
            | Self::Glint { phase }
            | Self::Breathe { phase }
            | Self::Wave { phase }
            | Self::Shine { phase }
            | Self::Ripple { phase, .. }
            | Self::Bob { phase, .. }
            | Self::Drift { phase, .. }
            | Self::Travel { phase, .. }
            | Self::Spin { phase, .. } => phase,
        }
    }

    /// The same movement started `offset` of a cycle later (wrapped into `[0, 1)`): nodes drawn with the
    /// same icon get their own offset ([`node_phase`]), so twins never blink in step. The loop stays seamless
    /// because only the phase moves, never the number of cycles.
    pub fn delayed(self, offset: f64) -> Self {
        let shift = |phase: f64| (phase + offset).rem_euclid(1.0);
        match self {
            Self::Blink { phase } => Self::Blink { phase: shift(phase) },
            Self::Glint { phase } => Self::Glint { phase: shift(phase) },
            Self::Breathe { phase } => Self::Breathe { phase: shift(phase) },
            Self::Wave { phase } => Self::Wave { phase: shift(phase) },
            Self::Shine { phase } => Self::Shine { phase: shift(phase) },
            Self::Ripple { center, phase } => Self::Ripple { center, phase: shift(phase) },
            Self::Spin { center, step_degrees, phase } => Self::Spin { center, step_degrees, phase: shift(phase) },
            Self::Bob { anchor, phase } => Self::Bob { anchor, phase: shift(phase) },
            Self::Drift { offset: sway, phase } => Self::Drift { offset: sway, phase: shift(phase) },
            Self::Travel { trip, phase } => Self::Travel { trip, phase: shift(phase) },
        }
    }

    /// Opacity over one cycle (before the phase), `None` when it stays as drawn.
    fn opacity_keys(&self) -> Option<Keyframes> {
        let keys = match self {
            Self::Blink { .. } => {
                vec![(0.0, 1.0), (0.5, 1.0), (0.56, BLINK_LOW), (0.74, BLINK_LOW), (0.8, 1.0), (1.0, 1.0)]
            }
            Self::Glint { .. } => {
                vec![(0.0, 0.0), (0.5, 0.0), (0.55, GLINT_PEAK), (0.7, GLINT_PEAK), (0.78, 0.0), (1.0, 0.0)]
            }
            Self::Breathe { .. } => return Some(Keyframes::smooth(BREATHE_LOW, 1.0)),
            Self::Wave { .. } => vec![(0.0, WAVE_LOW), (0.12, 1.0), (0.4, WAVE_LOW), (1.0, WAVE_LOW)],
            Self::Shine { .. } => vec![(0.0, 0.0), (0.3, 0.0), (0.5, SHINE_PEAK), (0.7, 0.0), (1.0, 0.0)],
            Self::Ripple { .. } => vec![(0.0, 0.0), (0.12, RIPPLE_PEAK), (0.8, 0.0), (1.0, 0.0)],
            Self::Travel { .. } => {
                vec![(0.0, 0.0), (TRAVEL_FADE, 1.0), (1.0 - TRAVEL_FADE, 1.0), (1.0, 0.0)]
            }
            Self::Spin { .. } | Self::Bob { .. } | Self::Drift { .. } => return None,
        };
        Some(Keyframes(keys))
    }

    /// Transform over one cycle (before the phase), `None` when the part does not move.
    pub fn transform_track(&self) -> Option<TransformTrack> {
        match *self {
            Self::Ripple { center, .. } => Some(TransformTrack::Scale {
                center,
                keys: Keyframes(vec![(0.0, RIPPLE_START_SCALE), (0.8, 1.0), (1.0, 1.0)]),
            }),
            Self::Spin { center, step_degrees, .. } => {
                Some(TransformTrack::FloorRotate { center, keys: Keyframes(vec![(0.0, 0.0), (1.0, step_degrees)]) })
            }
            Self::Bob { anchor, .. } => Some(TransformTrack::ScaleY { anchor, keys: Keyframes::smooth(BOB_LOW, 1.0) }),
            Self::Drift { offset, .. } => {
                // Cosine around the rest position, between −offset and +offset.
                Some(TransformTrack::Translate { direction: offset, keys: Keyframes::smooth(-1.0, 1.0) })
            }
            Self::Travel { trip, .. } => {
                Some(TransformTrack::Translate { direction: trip, keys: Keyframes(vec![(0.0, 0.0), (1.0, 1.0)]) })
            }
            _ => None,
        }
    }

    /// Opacity keyframes over one cycle, phase applied (what SMIL animates).
    pub fn opacity_track(&self) -> Option<Keyframes> {
        self.opacity_keys().map(|keys| keys.shifted(self.phase()))
    }

    /// Transform keyframes over one cycle, phase applied (what SMIL animates).
    pub fn phased_transform_track(&self) -> Option<TransformTrack> {
        let phase = self.phase();
        self.transform_track().map(|track| match track {
            TransformTrack::Translate { direction, keys } => {
                TransformTrack::Translate { direction, keys: keys.shifted(phase) }
            }
            TransformTrack::Scale { center, keys } => TransformTrack::Scale { center, keys: keys.shifted(phase) },
            TransformTrack::ScaleY { anchor, keys } => TransformTrack::ScaleY { anchor, keys: keys.shifted(phase) },
            TransformTrack::FloorRotate { center, keys } => {
                TransformTrack::FloorRotate { center, keys: keys.shifted(phase) }
            }
        })
    }

    /// State at loop time `t ∈ [0, 1)` of a loop lasting `duration_s`. `t = 1` gives the same as `t = 0`.
    pub fn state_at(&self, t: f64, duration_s: f64) -> MotionState {
        let cycles = f64::from(self.cycles(duration_s));
        let u = (t.rem_euclid(1.0) * cycles + self.phase()).rem_euclid(1.0);
        let opacity = self.opacity_keys().map_or(1.0, |keys| keys.value_at(u));
        let transform = self.transform_track().map_or(Affine::IDENTITY, |track| track.affine_at(u));
        MotionState { opacity, transform }
    }

    /// The `data-icon-motion` value: kind and numbers (3 decimals), parsed back by [`IconMotion::parse`].
    pub fn attribute(&self) -> String {
        let numbers: Vec<f64> = match *self {
            Self::Blink { phase }
            | Self::Glint { phase }
            | Self::Breathe { phase }
            | Self::Wave { phase }
            | Self::Shine { phase } => vec![phase],
            Self::Ripple { center, phase } => vec![center.x, center.y, phase],
            Self::Spin { center, step_degrees, phase } => vec![center.x, center.y, step_degrees, phase],
            Self::Bob { anchor, phase } => vec![anchor.x, anchor.y, phase],
            Self::Drift { offset, phase } => vec![offset.x, offset.y, phase],
            Self::Travel { trip, phase } => vec![trip.x, trip.y, phase],
        };
        let mut text = self.kind().to_owned();
        for number in numbers {
            let _ = write!(text, " {number:.3}");
        }
        text
    }

    fn kind(&self) -> &'static str {
        match self {
            Self::Blink { .. } => "blink",
            Self::Glint { .. } => "glint",
            Self::Breathe { .. } => "breathe",
            Self::Wave { .. } => "wave",
            Self::Shine { .. } => "shine",
            Self::Ripple { .. } => "ripple",
            Self::Spin { .. } => "spin",
            Self::Bob { .. } => "bob",
            Self::Drift { .. } => "drift",
            Self::Travel { .. } => "travel",
        }
    }

    /// Parses an [`IconMotion::attribute`] value.
    pub fn parse(text: &str) -> Option<Self> {
        let mut words = text.split_whitespace();
        let kind = words.next()?;
        let numbers: Vec<f64> = words.map(str::parse).collect::<Result<_, _>>().ok()?;
        let point = |x: f64, y: f64| ScreenPoint::new(x, y);
        let vector = |x: f64, y: f64| ScreenVector::new(x, y);
        Some(match (kind, &numbers[..]) {
            ("blink", &[phase]) => Self::Blink { phase },
            ("glint", &[phase]) => Self::Glint { phase },
            ("breathe", &[phase]) => Self::Breathe { phase },
            ("wave", &[phase]) => Self::Wave { phase },
            ("shine", &[phase]) => Self::Shine { phase },
            ("ripple", &[x, y, phase]) => Self::Ripple { center: point(x, y), phase },
            ("spin", &[x, y, step_degrees]) => Self::Spin { center: point(x, y), step_degrees, phase: 0.0 },
            ("spin", &[x, y, step_degrees, phase]) => Self::Spin { center: point(x, y), step_degrees, phase },
            ("bob", &[x, y, phase]) => Self::Bob { anchor: point(x, y), phase },
            ("drift", &[x, y, phase]) => Self::Drift { offset: vector(x, y), phase },
            ("travel", &[x, y, phase]) => Self::Travel { trip: vector(x, y), phase },
            _ => return None,
        })
    }
}

impl TransformTrack {
    /// Screen transform at cycle time `u` (keyframes already phased or not, as the caller chose).
    pub fn affine_at(&self, u: f64) -> Affine {
        match self {
            Self::Translate { direction, keys } => {
                let factor = keys.value_at(u);
                Affine { e: direction.x * factor, f: direction.y * factor, ..Affine::IDENTITY }
            }
            Self::Scale { center, keys } => about(*center, keys.value_at(u), keys.value_at(u)),
            Self::ScaleY { anchor, keys } => about(*anchor, 1.0, keys.value_at(u)),
            Self::FloorRotate { center, keys } => floor_rotation(*center, keys.value_at(u)),
        }
    }
}

/// Scale by `(sx, sy)` around `center`.
fn about(center: ScreenPoint, sx: f64, sy: f64) -> Affine {
    Affine { a: sx, b: 0.0, c: 0.0, d: sy, e: center.x * (1.0 - sx), f: center.y * (1.0 - sy) }
}

/// Floor projection (plan unit vectors → screen, tile 1): its inverse and itself conjugate a plan rotation.
pub const FLOOR_BASIS: Affine = Affine { a: COS_30, b: SIN_30, c: -COS_30, d: SIN_30, e: 0.0, f: 0.0 };

/// Inverse of [`FLOOR_BASIS`].
pub fn floor_basis_inverse() -> Affine {
    let determinant = FLOOR_BASIS.a * FLOOR_BASIS.d - FLOOR_BASIS.b * FLOOR_BASIS.c;
    Affine {
        a: FLOOR_BASIS.d / determinant,
        b: -FLOOR_BASIS.b / determinant,
        c: -FLOOR_BASIS.c / determinant,
        d: FLOOR_BASIS.a / determinant,
        e: 0.0,
        f: 0.0,
    }
}

/// A plan rotation by `degrees` around the screen point `center` of the floor plane, as a screen affine:
/// `translate(center) · basis · rotate · basis⁻¹ · translate(−center)`.
fn floor_rotation(center: ScreenPoint, degrees: f64) -> Affine {
    let (sin, cos) = degrees.to_radians().sin_cos();
    let rotate = Affine { a: cos, b: sin, c: -sin, d: cos, e: 0.0, f: 0.0 };
    let linear = compose(&compose(&FLOOR_BASIS, &rotate), &floor_basis_inverse());
    Affine {
        e: center.x - (linear.a * center.x + linear.c * center.y),
        f: center.y - (linear.b * center.x + linear.d * center.y),
        ..linear
    }
}

/// `outer · inner` (apply `inner` first).
fn compose(outer: &Affine, inner: &Affine) -> Affine {
    Affine {
        a: outer.a * inner.a + outer.c * inner.b,
        b: outer.b * inner.a + outer.d * inner.b,
        c: outer.a * inner.c + outer.c * inner.d,
        d: outer.b * inner.c + outer.d * inner.d,
        e: outer.a * inner.e + outer.c * inner.f + outer.e,
        f: outer.b * inner.e + outer.d * inner.f + outer.f,
    }
}

/// Opens a motion hook around the next drawing calls (close with [`close_hook`]), delayed by the canvas'
/// [`IsoCanvas::motion_phase`] (the node's own offset).
pub fn open_hook(canvas: &mut IsoCanvas, motion: &IconMotion) {
    let motion = motion.delayed(canvas.motion_phase());
    canvas.raw(&format!("<g {MOTION_ATTRIBUTE}=\"{}\">", motion.attribute()), Rect::EMPTY);
}

/// Phase offset (`[0, 1)`, a share of every cycle) of the icon movements of the node `id`: a hash of the id,
/// so it is deterministic and two nodes with the same icon (API #1, API #2) move out of step.
pub fn node_phase(id: &str) -> f64 {
    // FNV-1a: tiny, stable across platforms and releases (std's hasher is not guaranteed to be).
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let hash = id.bytes().fold(OFFSET_BASIS, |hash, byte| (hash ^ u64::from(byte)).wrapping_mul(PRIME));
    // FNV barely touches the top bits for ids that differ in the last byte ("api1", "api2"): a final
    // avalanche (MurmurHash3's fmix64) spreads them over the whole range.
    let mixed = fmix64(hash);
    let phase = (mixed >> 11) as f64 / (1u64 << 53) as f64;
    // The hook attribute keeps 3 decimals: round here so frames and SMIL read the very same phase.
    (phase * PHASE_STEPS).round().rem_euclid(PHASE_STEPS) / PHASE_STEPS
}

/// MurmurHash3's 64-bit finalizer: every input bit flips about half of the output bits.
fn fmix64(mut hash: u64) -> u64 {
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xff51_afd7_ed55_8ccd);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    hash ^ (hash >> 33)
}

/// Resolution of [`node_phase`] (the precision of the hook attribute).
const PHASE_STEPS: f64 = 1000.0;

/// Closes the hook opened by [`open_hook`].
pub fn close_hook(canvas: &mut IsoCanvas) {
    canvas.raw("</g>", Rect::EMPTY);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all_motions() -> Vec<IconMotion> {
        let point = ScreenPoint::new(10.0, -20.0);
        let vector = ScreenVector::new(12.0, 7.0);
        vec![
            IconMotion::Blink { phase: 0.25 },
            IconMotion::Glint { phase: 0.6 },
            IconMotion::Breathe { phase: 0.1 },
            IconMotion::Wave { phase: 0.3 },
            IconMotion::Shine { phase: 0.0 },
            IconMotion::Ripple { center: point, phase: 0.5 },
            IconMotion::Spin { center: point, step_degrees: 360.0 / 7.0, phase: 0.45 },
            IconMotion::Bob { anchor: point, phase: 0.4 },
            IconMotion::Drift { offset: vector, phase: 0.2 },
            IconMotion::Travel { trip: vector, phase: 0.75 },
        ]
    }

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-9
    }

    fn same_state(a: MotionState, b: MotionState) -> bool {
        let (p, q) = (a.transform, b.transform);
        close(a.opacity, b.opacity)
            && [(p.a, q.a), (p.b, q.b), (p.c, q.c), (p.d, q.d), (p.e, q.e), (p.f, q.f)]
                .iter()
                .all(|(x, y)| (x - y).abs() < 1e-6)
    }

    #[test]
    fn loops_are_seamless_for_any_duration() {
        for motion in all_motions() {
            for duration in [0.8, 2.0, 3.0, 5.5] {
                assert!(same_state(motion.state_at(0.0, duration), motion.state_at(1.0, duration)), "{motion:?}");
                assert!(motion.cycles(duration) >= 1);
            }
        }
        assert_eq!(IconMotion::Blink { phase: 0.0 }.cycles(2.0), 2);
        assert_eq!(IconMotion::Ripple { center: ScreenPoint::default(), phase: 0.0 }.cycles(2.0), 1);
    }

    #[test]
    fn delayed_motions_stay_seamless_and_keep_their_relative_phases() {
        for motion in all_motions() {
            let delayed = motion.delayed(0.37);
            for duration in [0.8, 2.0, 3.0] {
                assert!(same_state(delayed.state_at(0.0, duration), delayed.state_at(1.0, duration)), "{delayed:?}");
                assert_eq!(delayed.cycles(duration), motion.cycles(duration));
            }
            // Delaying by a whole cycle is no delay at all.
            assert!(same_state(motion.delayed(1.0).state_at(0.3, 2.0), motion.state_at(0.3, 2.0)), "{motion:?}");
        }
        let (first, second) = (IconMotion::Blink { phase: 0.1 }, IconMotion::Blink { phase: 0.6 });
        let gap = |a: IconMotion, b: IconMotion| (b.phase() - a.phase()).rem_euclid(1.0);
        assert!(close(gap(first.delayed(0.7), second.delayed(0.7)), gap(first, second)));
    }

    #[test]
    fn node_phases_are_deterministic_spread_and_in_range() {
        assert_eq!(node_phase("api1"), node_phase("api1"));
        let ids = ["api1", "api2", "api3", "db", "cache", "lb", "web-1", "web-2"];
        let phases: Vec<f64> = ids.iter().map(|id| node_phase(id)).collect();
        assert!(phases.iter().all(|phase| (0.0..1.0).contains(phase)), "{phases:?}");
        // Twins must not move in step: their phases differ by a visible share of the cycle.
        let apart = |a: f64, b: f64| {
            let gap = (a - b).rem_euclid(1.0);
            gap.min(1.0 - gap)
        };
        assert!(apart(node_phase("api1"), node_phase("api2")) > 0.1, "{phases:?}");
        assert!(apart(node_phase("web-1"), node_phase("web-2")) > 0.1, "{phases:?}");
        assert_eq!((node_phase("api1") * PHASE_STEPS).fract(), 0.0, "rounded to the attribute precision");
    }

    #[test]
    fn hooks_carry_the_canvas_phase() {
        let mut canvas = IsoCanvas::new(crate::geometry::Projection::default());
        canvas.set_motion_phase(0.25);
        open_hook(&mut canvas, &IconMotion::Blink { phase: 0.5 });
        close_hook(&mut canvas);
        assert!(canvas.finish().body.contains("data-icon-motion=\"blink 0.750\""));
    }

    #[test]
    fn spin_by_one_symmetry_step_closes_the_loop() {
        let spin = IconMotion::Spin { center: ScreenPoint::new(5.0, 5.0), step_degrees: 90.0, phase: 0.0 };
        let quarter = spin.state_at(0.999_999_999, 2.0).transform;
        // A quarter turn on the floor maps the plan x axis onto the plan y axis.
        let x_axis = (FLOOR_BASIS.a, FLOOR_BASIS.b);
        let turned = (quarter.a * x_axis.0 + quarter.c * x_axis.1, quarter.b * x_axis.0 + quarter.d * x_axis.1);
        assert!((turned.0 - FLOOR_BASIS.c).abs() < 1e-6 && (turned.1 - FLOOR_BASIS.d).abs() < 1e-6, "{turned:?}");
        // The axis point stays put.
        let center = spin.state_at(0.37, 2.0).transform;
        assert!(close(center.a * 5.0 + center.c * 5.0 + center.e, 5.0));
    }

    #[test]
    fn overlays_are_invisible_at_rest_and_amplitudes_are_small() {
        for motion in all_motions() {
            let samples: Vec<MotionState> =
                (0..100).map(|step| motion.state_at(f64::from(step) / 100.0, 2.0)).collect();
            assert!(samples.iter().all(|state| (0.0..=1.0).contains(&state.opacity)));
            if motion.is_overlay() {
                assert!(samples.iter().any(|state| state.opacity == 0.0), "{motion:?} hides between flashes");
            } else {
                assert!(samples.iter().any(|state| close(state.opacity, 1.0)), "{motion:?} returns to its drawing");
            }
        }
        let drift = IconMotion::Drift { offset: ScreenVector::new(1.5, 0.0), phase: 0.0 };
        let largest =
            (0..100).map(|step| drift.state_at(f64::from(step) / 100.0, 2.0).transform.e.abs()).fold(0.0, f64::max);
        assert!(largest <= 1.5 + 1e-9);
    }

    #[test]
    fn attributes_round_trip() {
        for motion in all_motions() {
            let parsed = IconMotion::parse(&motion.attribute()).unwrap_or_else(|| panic!("{motion:?}"));
            assert_eq!(parsed.attribute(), motion.attribute());
        }
        assert_eq!(IconMotion::parse("blink"), None);
        assert_eq!(IconMotion::parse("teleport 1"), None);
    }

    #[test]
    fn shifted_keyframes_match_the_phased_function() {
        for motion in all_motions() {
            let Some(keys) = motion.opacity_keys() else { continue };
            let shifted = keys.shifted(motion.phase());
            for step in 0..200 {
                let u = f64::from(step) / 200.0;
                let expected = keys.value_at((u + motion.phase()).rem_euclid(1.0));
                assert!(close(shifted.value_at(u), expected), "{motion:?} at {u}");
            }
            let times: Vec<f64> = shifted.points().iter().map(|point| point.0).collect();
            assert!(
                times.windows(2).all(|pair| pair[0] <= pair[1]) && times[0] == 0.0 && times[times.len() - 1] == 1.0
            );
        }
    }
}
