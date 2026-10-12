//! Plan (tiles) and screen (px) geometry, true isometric projection (30°), floor/face matrices,
//! camera rotation and Bézier helpers. See `docs/06-renderizacao.md#projeção`.
//!
//! Conventions (rotation 0): plan `x` grows down-right on screen, plan `y` grows down-left, `z` is height.
//! `X = (x − y)·cos30°·T`, `Y = (x + y)·sin30°·T − z·T`.

use std::ops::{Add, Mul, Sub};

use dioramic_spec::Rotation;

/// cos 30°.
pub const COS_30: f64 = 0.866_025_403_784_438_6;
/// sin 30°.
pub const SIN_30: f64 = 0.5;
/// Default tile size in px (prototype `T = 56`).
pub const DEFAULT_TILE_PX: f64 = 56.0;

/// A point on the floor plan, in tiles.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlanPoint {
    /// Plan x.
    pub x: f64,
    /// Plan y.
    pub y: f64,
}

/// A direction/offset on the floor plan, in tiles.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct PlanVector {
    /// Δx.
    pub x: f64,
    /// Δy.
    pub y: f64,
}

/// A point on screen, in px (SVG user units).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScreenPoint {
    /// Screen X (right).
    pub x: f64,
    /// Screen Y (down).
    pub y: f64,
}

/// A direction/offset on screen, in px.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ScreenVector {
    /// ΔX.
    pub x: f64,
    /// ΔY.
    pub y: f64,
}

impl PlanPoint {
    /// New point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Vector from `self` to `other`.
    pub fn to(self, other: PlanPoint) -> PlanVector {
        PlanVector::new(other.x - self.x, other.y - self.y)
    }

    /// Euclidean distance in tiles.
    pub fn distance(self, other: PlanPoint) -> f64 {
        self.to(other).length()
    }
}

impl PlanVector {
    /// New vector.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Length in tiles.
    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Unit vector; a zero vector stays zero (no NaN).
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length <= f64::EPSILON { Self::default() } else { Self::new(self.x / length, self.y / length) }
    }

    /// Perpendicular (rotated +90° in the plan: `(−y, x)`).
    pub fn perpendicular(self) -> Self {
        Self::new(-self.y, self.x)
    }

    /// Dot product.
    pub fn dot(self, other: PlanVector) -> f64 {
        self.x * other.x + self.y * other.y
    }
}

impl ScreenPoint {
    /// New point.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Vector from `self` to `other`.
    pub fn to(self, other: ScreenPoint) -> ScreenVector {
        ScreenVector::new(other.x - self.x, other.y - self.y)
    }

    /// Euclidean distance in px.
    pub fn distance(self, other: ScreenPoint) -> f64 {
        self.to(other).length()
    }

    /// Midpoint.
    pub fn midpoint(self, other: ScreenPoint) -> Self {
        Self::new((self.x + other.x) / 2.0, (self.y + other.y) / 2.0)
    }
}

impl ScreenVector {
    /// New vector.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    /// Length in px.
    pub fn length(self) -> f64 {
        (self.x * self.x + self.y * self.y).sqrt()
    }

    /// Unit vector; a zero vector stays zero.
    pub fn normalized(self) -> Self {
        let length = self.length();
        if length <= f64::EPSILON { Self::default() } else { Self::new(self.x / length, self.y / length) }
    }

    /// Angle in degrees, `atan2(y, x)`.
    pub fn angle_degrees(self) -> f64 {
        self.y.atan2(self.x).to_degrees()
    }

    /// Smallest absolute angle between two directions, in degrees (0..=180).
    pub fn angle_between_degrees(self, other: ScreenVector) -> f64 {
        let difference = (self.angle_degrees() - other.angle_degrees()).rem_euclid(360.0);
        difference.min(360.0 - difference)
    }
}

macro_rules! impl_vector_ops {
    ($point:ty, $vector:ty) => {
        impl Add<$vector> for $point {
            type Output = $point;
            fn add(self, rhs: $vector) -> $point {
                <$point>::new(self.x + rhs.x, self.y + rhs.y)
            }
        }
        impl Sub<$vector> for $point {
            type Output = $point;
            fn sub(self, rhs: $vector) -> $point {
                <$point>::new(self.x - rhs.x, self.y - rhs.y)
            }
        }
        impl Sub for $point {
            type Output = $vector;
            fn sub(self, rhs: $point) -> $vector {
                <$vector>::new(self.x - rhs.x, self.y - rhs.y)
            }
        }
        impl Add for $vector {
            type Output = $vector;
            fn add(self, rhs: $vector) -> $vector {
                <$vector>::new(self.x + rhs.x, self.y + rhs.y)
            }
        }
        impl Sub for $vector {
            type Output = $vector;
            fn sub(self, rhs: $vector) -> $vector {
                <$vector>::new(self.x - rhs.x, self.y - rhs.y)
            }
        }
        impl Mul<f64> for $vector {
            type Output = $vector;
            fn mul(self, rhs: f64) -> $vector {
                <$vector>::new(self.x * rhs, self.y * rhs)
            }
        }
    };
}
impl_vector_ops!(PlanPoint, PlanVector);
impl_vector_ops!(ScreenPoint, ScreenVector);

/// Axis-aligned rectangle on screen (px). `EMPTY` is the identity of [`Rect::union`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    /// Left.
    pub x0: f64,
    /// Top.
    pub y0: f64,
    /// Right.
    pub x1: f64,
    /// Bottom.
    pub y1: f64,
}

impl Default for Rect {
    fn default() -> Self {
        Self::EMPTY
    }
}

impl Rect {
    /// Empty rectangle (inverted infinities); growing it with a point yields that point.
    pub const EMPTY: Rect = Rect { x0: f64::INFINITY, y0: f64::INFINITY, x1: f64::NEG_INFINITY, y1: f64::NEG_INFINITY };

    /// From corners (normalizes order).
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }

    /// From top-left and size.
    pub fn from_origin_size(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self::new(x, y, x + width, y + height)
    }

    /// Centered on `center` with the given size.
    pub fn centered(center: ScreenPoint, width: f64, height: f64) -> Self {
        Self::from_origin_size(center.x - width / 2.0, center.y - height / 2.0, width, height)
    }

    /// Bounding box of points (EMPTY for none).
    pub fn from_points(points: impl IntoIterator<Item = ScreenPoint>) -> Self {
        points.into_iter().fold(Self::EMPTY, Self::including)
    }

    /// True when nothing was added.
    pub fn is_empty(&self) -> bool {
        self.x0 > self.x1 || self.y0 > self.y1
    }

    /// Width (0 when empty).
    pub fn width(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.x1 - self.x0 }
    }

    /// Height (0 when empty).
    pub fn height(&self) -> f64 {
        if self.is_empty() { 0.0 } else { self.y1 - self.y0 }
    }

    /// Area (0 when empty).
    pub fn area(&self) -> f64 {
        self.width() * self.height()
    }

    /// Center point.
    pub fn center(&self) -> ScreenPoint {
        ScreenPoint::new((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    /// Grows to include a point.
    pub fn include(&mut self, point: ScreenPoint) {
        *self = self.including(point);
    }

    /// Copy grown to include a point.
    pub fn including(self, point: ScreenPoint) -> Self {
        Self { x0: self.x0.min(point.x), y0: self.y0.min(point.y), x1: self.x1.max(point.x), y1: self.y1.max(point.y) }
    }

    /// Smallest rect containing both (empty rects are ignored).
    pub fn union(self, other: Rect) -> Self {
        if other.is_empty() {
            return self;
        }
        if self.is_empty() {
            return other;
        }
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Expanded by `margin` on every side (negative shrinks).
    pub fn inflate(self, margin: f64) -> Self {
        if self.is_empty() {
            return self;
        }
        Self { x0: self.x0 - margin, y0: self.y0 - margin, x1: self.x1 + margin, y1: self.y1 + margin }
    }

    /// Translated copy.
    pub fn translate(self, offset: ScreenVector) -> Self {
        Self { x0: self.x0 + offset.x, y0: self.y0 + offset.y, x1: self.x1 + offset.x, y1: self.y1 + offset.y }
    }

    /// True when the point is inside or on the border.
    pub fn contains(&self, point: ScreenPoint) -> bool {
        point.x >= self.x0 && point.x <= self.x1 && point.y >= self.y0 && point.y <= self.y1
    }

    /// Overlap area of the two rectangles after inflating the overlap by `margin` on each axis
    /// (prototype `hit(a, b, m)`: rects closer than `margin` px count as touching). 0 = no collision.
    pub fn overlap_area(&self, other: &Rect, margin: f64) -> f64 {
        if self.is_empty() || other.is_empty() {
            return 0.0;
        }
        let width = (self.x1.min(other.x1) - self.x0.max(other.x0) + margin).max(0.0);
        let height = (self.y1.min(other.y1) - self.y0.max(other.y0) + margin).max(0.0);
        width * height
    }

    /// True when [`Rect::overlap_area`] with margin 0 is positive.
    pub fn intersects(&self, other: &Rect) -> bool {
        self.overlap_area(other, 0.0) > 0.0
    }
}

/// Axis-aligned rectangle on the plan (tiles): zones, footprints.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlanRect {
    /// Min x.
    pub x0: f64,
    /// Min y.
    pub y0: f64,
    /// Max x.
    pub x1: f64,
    /// Max y.
    pub y1: f64,
}

impl PlanRect {
    /// From corners (normalizes order).
    pub fn new(x0: f64, y0: f64, x1: f64, y1: f64) -> Self {
        Self { x0: x0.min(x1), y0: y0.min(y1), x1: x0.max(x1), y1: y0.max(y1) }
    }

    /// Centered rect with half sizes.
    pub fn around(center: PlanPoint, half_width: f64, half_depth: f64) -> Self {
        Self::new(center.x - half_width, center.y - half_depth, center.x + half_width, center.y + half_depth)
    }

    /// Width along x.
    pub fn width(&self) -> f64 {
        self.x1 - self.x0
    }

    /// Depth along y.
    pub fn depth(&self) -> f64 {
        self.y1 - self.y0
    }

    /// Center.
    pub fn center(&self) -> PlanPoint {
        PlanPoint::new((self.x0 + self.x1) / 2.0, (self.y0 + self.y1) / 2.0)
    }

    /// Corners in drawing order: back `(x0,y0)`, right `(x1,y0)`, front `(x1,y1)`, left `(x0,y1)`.
    pub fn corners(&self) -> [PlanPoint; 4] {
        [
            PlanPoint::new(self.x0, self.y0),
            PlanPoint::new(self.x1, self.y0),
            PlanPoint::new(self.x1, self.y1),
            PlanPoint::new(self.x0, self.y1),
        ]
    }

    /// Smallest rect containing both.
    pub fn union(self, other: PlanRect) -> Self {
        Self {
            x0: self.x0.min(other.x0),
            y0: self.y0.min(other.y0),
            x1: self.x1.max(other.x1),
            y1: self.y1.max(other.y1),
        }
    }

    /// Expanded by `margin` tiles on every side.
    pub fn inflate(self, margin: f64) -> Self {
        Self { x0: self.x0 - margin, y0: self.y0 - margin, x1: self.x1 + margin, y1: self.y1 + margin }
    }

    /// True when the point is inside or on the border.
    pub fn contains(&self, point: PlanPoint) -> bool {
        point.x >= self.x0 && point.x <= self.x1 && point.y >= self.y0 && point.y <= self.y1
    }

    /// Rotated about `pivot` by the camera rotation, renormalized (corners rotated, then min/max).
    pub fn rotated(self, pivot: PlanPoint, rotation: Rotation) -> Self {
        let a = rotate_about(PlanPoint::new(self.x0, self.y0), pivot, rotation);
        let b = rotate_about(PlanPoint::new(self.x1, self.y1), pivot, rotation);
        Self::new(a.x, a.y, b.x, b.y)
    }
}

/// Rotates a plan point about `pivot` by `rotation`, applying `(x, y) → (−y, x)` once per quarter turn
/// (the prototype's `rotateScene`).
pub fn rotate_about(point: PlanPoint, pivot: PlanPoint, rotation: Rotation) -> PlanPoint {
    let mut x = point.x - pivot.x;
    let mut y = point.y - pivot.y;
    for _ in 0..rotation.quarter_turns() {
        (x, y) = (-y, x);
    }
    PlanPoint::new(x + pivot.x, y + pivot.y)
}

/// Rotates a plan direction by `rotation` (no pivot).
pub fn rotate_vector(vector: PlanVector, rotation: Rotation) -> PlanVector {
    let rotated = rotate_about(PlanPoint::new(vector.x, vector.y), PlanPoint::default(), rotation);
    PlanVector::new(rotated.x, rotated.y)
}

/// The isometric projection for a tile size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projection {
    /// Tile size in px.
    pub tile: f64,
}

impl Default for Projection {
    fn default() -> Self {
        Self { tile: DEFAULT_TILE_PX }
    }
}

impl Projection {
    /// Projection with tile size `tile` px.
    pub fn new(tile: f64) -> Self {
        Self { tile }
    }

    /// Plan point at height `z` (tiles) → screen.
    pub fn project(&self, point: PlanPoint, z: f64) -> ScreenPoint {
        ScreenPoint::new(
            (point.x - point.y) * COS_30 * self.tile,
            (point.x + point.y) * SIN_30 * self.tile - z * self.tile,
        )
    }

    /// Inverse of [`Projection::project`] for a known height `z` (desktop drag: screen → plan).
    pub fn unproject(&self, point: ScreenPoint, z: f64) -> PlanPoint {
        let lifted_y = point.y + z * self.tile;
        let a = point.x / (COS_30 * self.tile);
        let b = lifted_y / (SIN_30 * self.tile);
        PlanPoint::new((a + b) / 2.0, (b - a) / 2.0)
    }

    /// Plan direction → screen direction (not normalized; length in px for a vector in tiles).
    pub fn project_vector(&self, vector: PlanVector) -> ScreenVector {
        ScreenVector::new((vector.x - vector.y) * COS_30 * self.tile, (vector.x + vector.y) * SIN_30 * self.tile)
    }

    /// Screen radii `(rx, ry)` of a plan circle of radius `radius` tiles (`rx ≈ 1.225·r·T`, `ry ≈ 0.707·r·T`).
    pub fn circle_radii(&self, radius: f64) -> (f64, f64) {
        (std::f64::consts::SQRT_2 * COS_30 * radius * self.tile, std::f64::consts::SQRT_2 * SIN_30 * radius * self.tile)
    }
}

/// 2D affine transform as in SVG `matrix(a, b, c, d, e, f)`: `X = a·x + c·y + e`, `Y = b·x + d·y + f`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    /// a.
    pub a: f64,
    /// b.
    pub b: f64,
    /// c.
    pub c: f64,
    /// d.
    pub d: f64,
    /// e (translation X).
    pub e: f64,
    /// f (translation Y).
    pub f: f64,
}

impl Affine {
    /// Identity.
    pub const IDENTITY: Affine = Affine { a: 1.0, b: 0.0, c: 0.0, d: 1.0, e: 0.0, f: 0.0 };

    /// Lays 2D content (1 unit = 1 px) flat on the floor with text running along plan `+x`
    /// ("down-right"): `matrix(cos30, sin30, −cos30, sin30, X, Y)`.
    pub fn floor_along_x(origin: ScreenPoint) -> Self {
        Self { a: COS_30, b: SIN_30, c: -COS_30, d: SIN_30, e: origin.x, f: origin.y }
    }

    /// Lays 2D content flat on the floor with text running along plan `−y` ("up-right"):
    /// `matrix(cos30, −sin30, cos30, sin30, X, Y)`.
    pub fn floor_along_y(origin: ScreenPoint) -> Self {
        Self { a: COS_30, b: -SIN_30, c: COS_30, d: SIN_30, e: origin.x, f: origin.y }
    }

    /// Left face (constant y, facing +y): `matrix(cos30, sin30, 0, 1, X, Y)`.
    pub fn left_face(origin: ScreenPoint) -> Self {
        Self { a: COS_30, b: SIN_30, c: 0.0, d: 1.0, e: origin.x, f: origin.y }
    }

    /// Right face (constant x, facing +x): `matrix(cos30, −sin30, 0, 1, X, Y)`.
    pub fn right_face(origin: ScreenPoint) -> Self {
        Self { a: COS_30, b: -SIN_30, c: 0.0, d: 1.0, e: origin.x, f: origin.y }
    }

    /// Applies the transform to a local point.
    pub fn apply(&self, x: f64, y: f64) -> ScreenPoint {
        ScreenPoint::new(self.a * x + self.c * y + self.e, self.b * x + self.d * y + self.f)
    }

    /// Screen bounding box of a local rectangle `[0,w]×[0,h]` after the transform.
    pub fn bounds_of(&self, width: f64, height: f64) -> Rect {
        Rect::from_points([
            self.apply(0.0, 0.0),
            self.apply(width, 0.0),
            self.apply(width, height),
            self.apply(0.0, height),
        ])
    }
}

/// Cubic Bézier in screen space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CubicBezier {
    /// Start, control 1, control 2, end.
    pub points: [ScreenPoint; 4],
}

impl CubicBezier {
    /// New curve.
    pub fn new(start: ScreenPoint, control1: ScreenPoint, control2: ScreenPoint, end: ScreenPoint) -> Self {
        Self { points: [start, control1, control2, end] }
    }

    /// Point at parameter `t ∈ [0, 1]`.
    pub fn point_at(&self, t: f64) -> ScreenPoint {
        let [p0, p1, p2, p3] = self.points;
        let m = 1.0 - t;
        let (w0, w1, w2, w3) = (m * m * m, 3.0 * m * m * t, 3.0 * m * t * t, t * t * t);
        ScreenPoint::new(w0 * p0.x + w1 * p1.x + w2 * p2.x + w3 * p3.x, w0 * p0.y + w1 * p1.y + w2 * p2.y + w3 * p3.y)
    }

    /// Derivative (tangent, not normalized) at `t`.
    pub fn tangent_at(&self, t: f64) -> ScreenVector {
        let [p0, p1, p2, p3] = self.points;
        let m = 1.0 - t;
        let (w0, w1, w2) = (3.0 * m * m, 6.0 * m * t, 3.0 * t * t);
        ScreenVector::new(
            w0 * (p1.x - p0.x) + w1 * (p2.x - p1.x) + w2 * (p3.x - p2.x),
            w0 * (p1.y - p0.y) + w1 * (p2.y - p1.y) + w2 * (p3.y - p2.y),
        )
    }

    /// Direction in which the curve arrives at its end (`p3 − p2`, or the tangent if degenerate).
    pub fn end_direction(&self) -> ScreenVector {
        let direction = self.points[3] - self.points[2];
        if direction.length() > f64::EPSILON { direction } else { self.tangent_at(1.0) }
    }

    /// Arc-length table with `samples` segments, for uniform-speed motion and label sliding.
    pub fn arc_length_table(&self, samples: usize) -> ArcLengthTable {
        let samples = samples.max(1);
        let mut cumulative = Vec::with_capacity(samples + 1);
        cumulative.push(0.0);
        let mut previous = self.point_at(0.0);
        let mut total = 0.0;
        for index in 1..=samples {
            let point = self.point_at(index as f64 / samples as f64);
            total += previous.distance(point);
            cumulative.push(total);
            previous = point;
        }
        ArcLengthTable { cumulative }
    }
}

/// Cumulative arc length at evenly spaced `t` samples (prototype `arcLUT`).
#[derive(Debug, Clone, PartialEq)]
pub struct ArcLengthTable {
    cumulative: Vec<f64>,
}

impl ArcLengthTable {
    /// Total length in px.
    pub fn total_length(&self) -> f64 {
        self.cumulative.last().copied().unwrap_or(0.0)
    }

    /// Bézier parameter `t` at `fraction ∈ [0, 1]` of the total length (prototype `atDistance`).
    pub fn t_at_fraction(&self, fraction: f64) -> f64 {
        let segments = self.cumulative.len().saturating_sub(1);
        if segments == 0 {
            return 0.0;
        }
        let target = fraction.clamp(0.0, 1.0) * self.total_length();
        let upper = self.cumulative.partition_point(|&length| length < target).clamp(1, segments);
        let start = self.cumulative[upper - 1];
        let span = self.cumulative[upper] - start;
        let within = if span > f64::EPSILON { (target - start) / span } else { 0.0 };
        ((upper - 1) as f64 + within) / segments as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPS: f64 = 1e-9;

    fn close(a: f64, b: f64) -> bool {
        (a - b).abs() < 1e-6
    }

    #[test]
    fn projection_matches_doc_formula() {
        let projection = Projection::new(56.0);
        let point = projection.project(PlanPoint::new(2.0, 1.0), 0.5);
        assert!(close(point.x, 1.0 * COS_30 * 56.0));
        assert!(close(point.y, 3.0 * 0.5 * 56.0 - 0.5 * 56.0));
    }

    #[test]
    fn grid_lines_have_iso_slope() {
        let projection = Projection::default();
        let a = projection.project(PlanPoint::new(0.0, 0.0), 0.0);
        let b = projection.project(PlanPoint::new(1.0, 0.0), 0.0);
        let slope = (b.y - a.y) / (b.x - a.x);
        assert!(close(slope, (30f64).to_radians().tan()));
    }

    #[test]
    fn unproject_inverts_project() {
        let projection = Projection::new(40.0);
        for &(x, y, z) in &[(0.0, 0.0, 0.0), (3.5, -2.0, 0.0), (7.25, 4.5, 1.2)] {
            let back = projection.unproject(projection.project(PlanPoint::new(x, y), z), z);
            assert!(close(back.x, x) && close(back.y, y), "{x},{y},{z} -> {back:?}");
        }
    }

    #[test]
    fn circle_radii_ratio_is_tan30() {
        let (rx, ry) = Projection::new(56.0).circle_radii(0.5);
        assert!(close(ry / rx, (30f64).to_radians().tan()));
        assert!(close(rx, 1.224_744_871 * 0.5 * 56.0));
    }

    #[test]
    fn rotation_quarter_turn_maps_x_to_y() {
        let pivot = PlanPoint::new(1.0, 1.0);
        let rotated = rotate_about(PlanPoint::new(2.0, 1.0), pivot, Rotation::R90);
        assert!(close(rotated.x, 1.0) && close(rotated.y, 2.0));
        let full = rotate_about(PlanPoint::new(5.0, -3.0), pivot, Rotation::R180);
        let back = rotate_about(full, pivot, Rotation::R180);
        assert!(close(back.x, 5.0) && close(back.y, -3.0));
    }

    #[test]
    fn plan_rect_rotation_swaps_dimensions() {
        let rect = PlanRect::new(0.0, 0.0, 4.0, 2.0);
        let rotated = rect.rotated(PlanPoint::new(2.0, 1.0), Rotation::R90);
        assert!(close(rotated.width(), 2.0) && close(rotated.depth(), 4.0));
        assert!(close(rotated.center().x, 2.0) && close(rotated.center().y, 1.0));
    }

    #[test]
    fn rect_union_and_overlap() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(5.0, 5.0, 15.0, 15.0);
        assert!(close(a.overlap_area(&b, 0.0), 25.0));
        assert!(close(a.overlap_area(&Rect::new(20.0, 0.0, 30.0, 10.0), 0.0), 0.0));
        // touching within the margin counts (prototype m = 3)
        assert!(a.overlap_area(&Rect::new(11.0, 0.0, 20.0, 10.0), 3.0) > 0.0);
        let union = Rect::EMPTY.union(a).union(b);
        assert_eq!(union, Rect::new(0.0, 0.0, 15.0, 15.0));
        assert!(Rect::EMPTY.is_empty());
        assert!(close(Rect::EMPTY.area(), 0.0));
    }

    #[test]
    fn floor_matrix_follows_grid() {
        let floor = Affine::floor_along_x(ScreenPoint::new(0.0, 0.0));
        let projection = Projection::new(1.0);
        let along = floor.apply(1.0, 0.0);
        let expected = projection.project(PlanPoint::new(1.0, 0.0), 0.0);
        assert!(close(along.x, expected.x) && close(along.y, expected.y));
    }

    #[test]
    fn bezier_endpoints_and_arc_length() {
        let line = CubicBezier::new(
            ScreenPoint::new(0.0, 0.0),
            ScreenPoint::new(10.0, 0.0),
            ScreenPoint::new(20.0, 0.0),
            ScreenPoint::new(30.0, 0.0),
        );
        assert!(line.point_at(0.0).distance(ScreenPoint::new(0.0, 0.0)) < EPS);
        assert!(line.point_at(1.0).distance(ScreenPoint::new(30.0, 0.0)) < EPS);
        let table = line.arc_length_table(200);
        assert!(close(table.total_length(), 30.0));
        let half = line.point_at(table.t_at_fraction(0.5));
        assert!((half.x - 15.0).abs() < 0.1);
        assert!(close(line.end_direction().angle_degrees(), 0.0));
    }

    #[test]
    fn angle_between_wraps() {
        let a = ScreenVector::new(1.0, 0.0);
        let b = ScreenVector::new(1.0, -0.0001);
        assert!(a.angle_between_degrees(b) < 0.01);
        assert!(close(a.angle_between_degrees(ScreenVector::new(-1.0, 0.0)), 180.0));
    }
}
