//! Isometric primitives shared by the procedural icons (`docs/06-renderizacao.md#primitivas`).
//!
//! `iso_box`, `cylinder` and `shadow` are ported from the prototype (`docs/spike/scene.mjs`). The rest
//! (slab, stack, tube, dome, sphere, cloud, person, screen, prism) work in an [`IconFrame`]: icon-local
//! coordinates in tiles, where the icon's *front* is the local `+b` face. On odd camera rotations the frame
//! swaps the plan axes, so non-square icons turn with the world and their front stays on a visible face.

use super::motion::{IconMotion, close_hook, open_hook};
use crate::canvas::{IsoCanvas, LineCap, Paint, PathData, Stroke, Style, TextAnchor, TextStyle};
use crate::color::Color;
use crate::geometry::{DEFAULT_TILE_PX, PlanPoint, Projection, Rect, ScreenPoint, ScreenVector};
use crate::theme::{IconFinish, Theme};
use dioramic_spec::Rotation;

/// Plan offset of the shadow relative to the node center (light from the top-left).
const SHADOW_OFFSET: (f64, f64) = (0.12, -0.05);
/// Shading of a slanted face that looks mostly up (sign plates, bucket rims).
const SLANTED_TOP_SHADE: f64 = 0.9;
/// Highlight of the top ellipse of cylinders (prototype: ×1.12).
const CYLINDER_TOP_SHADE: f64 = 1.12;
/// How far prism faces may extrapolate beyond the right/left shading (factors of each).
const PRISM_SHADE_SPREAD: (f64, f64) = (0.85, 1.1);
/// Samples of a circle when it is drawn as a polygon (lying tubes).
const CIRCLE_SAMPLES: usize = 48;
/// Puffs of the cloud at the default tile, `(dx, dy, r)` px around its center (prototype).
const CLOUD_PUFFS: [(f64, f64, f64); 5] =
    [(-22.0, 6.0, 17.0), (0.0, -6.0, 23.0), (22.0, 4.0, 18.0), (-6.0, 12.0, 16.0), (12.0, 13.0, 15.0)];
/// Height (tiles) of the cloud puffs' center.
const CLOUD_CENTER_Z: f64 = 0.45;
/// Approximate advance of a bold badge glyph, as a fraction of the font size (Inter 700).
const BADGE_GLYPH_WIDTH: f64 = 0.62;
/// Badge font size (px).
const BADGE_FONT_SIZE: f64 = 10.0;
/// Badge pill height (px).
const BADGE_HEIGHT: f64 = 16.0;
/// Badge horizontal padding (px each side).
const BADGE_PADDING: f64 = 6.0;
/// Glare bands on a silver glass: `((u_left, u_right) at the top, opacity)`.
const GLASS_GLARE: [((f64, f64), f64); 2] = [((0.5, 0.78), 0.45), ((0.84, 0.92), 0.6)];
/// Horizontal run of a glare band from the top to the bottom of the glass (fraction of its width).
const GLARE_SLANT: f64 = 0.38;

fn outline(theme: &Theme) -> Stroke {
    Stroke::new(theme.stroke, theme.stroke_width)
}

fn outlined(theme: &Theme, color: Color) -> Style {
    Style::fill(color).with_stroke(outline(theme))
}

/// Box with min corner `origin` (plan), size `width × depth × height` tiles, base at `z0`.
/// Draws the three visible faces: left (`y + depth`), right (`x + width`) and top, shaded by the theme.
pub fn iso_box(canvas: &mut IsoCanvas, theme: &Theme, origin: PlanPoint, size: [f64; 3], z0: f64, color: Color) {
    let [width, depth, height] = size;
    let (x0, y0, x1, y1) = (origin.x, origin.y, origin.x + width, origin.y + depth);
    let top_z = z0 + height;
    let face = |shade: f64| outlined(theme, color.shade(shade));
    let left = [
        canvas.project(PlanPoint::new(x0, y1), z0),
        canvas.project(PlanPoint::new(x1, y1), z0),
        canvas.project(PlanPoint::new(x1, y1), top_z),
        canvas.project(PlanPoint::new(x0, y1), top_z),
    ];
    canvas.polygon(&left, &face(theme.faces.left));
    let right = [
        canvas.project(PlanPoint::new(x1, y0), z0),
        canvas.project(PlanPoint::new(x1, y1), z0),
        canvas.project(PlanPoint::new(x1, y1), top_z),
        canvas.project(PlanPoint::new(x1, y0), top_z),
    ];
    canvas.polygon(&right, &face(theme.faces.right));
    canvas.plan_polygon(
        &[PlanPoint::new(x0, y0), PlanPoint::new(x1, y0), PlanPoint::new(x1, y1), PlanPoint::new(x0, y1)],
        top_z,
        &face(theme.faces.top),
    );
}

/// Registers the horizontal side gradient of a cylinder of `color` and returns its id.
pub fn side_gradient(canvas: &mut IsoCanvas, color: Color) -> String {
    let gradient_id = format!("cyl{}", &color.to_hex()[1..]);
    let gradient = format!(
        "<linearGradient id=\"{gradient_id}\" x1=\"0\" x2=\"1\"><stop offset=\"0\" stop-color=\"{}\"/><stop offset=\"0.55\" stop-color=\"{}\"/><stop offset=\"1\" stop-color=\"{}\"/></linearGradient>",
        color.shade(0.95),
        color.shade(0.78),
        color.shade(0.55)
    );
    canvas.define(gradient_id.clone(), gradient)
}

/// Registers a spherical (radial, light from the top-left) gradient and returns its id.
fn sphere_gradient(canvas: &mut IsoCanvas, light: Color, dark: Color) -> String {
    let gradient_id = format!("sph{}{}", &light.to_hex()[1..], &dark.to_hex()[1..]);
    let gradient = format!(
        "<radialGradient id=\"{gradient_id}\" cx=\"0.35\" cy=\"0.3\" r=\"0.8\"><stop offset=\"0\" stop-color=\"{light}\"/><stop offset=\"1\" stop-color=\"{dark}\"/></radialGradient>"
    );
    canvas.define(gradient_id.clone(), gradient)
}

/// Vertical cylinder centered at `center`, radius in tiles, with a horizontal gradient side and a lighter top.
pub fn cylinder(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    center: PlanPoint,
    radius: f64,
    height: f64,
    z0: f64,
    color: Color,
) {
    let gradient_id = side_gradient(canvas, color);
    let bottom = canvas.project_quiet(center, z0);
    let top = canvas.project_quiet(center, z0 + height);
    let (radius_x, radius_y) = canvas.projection().circle_radii(radius);
    let extent = Rect::new(bottom.x - radius_x, top.y - radius_y, bottom.x + radius_x, bottom.y + radius_y);
    let side = PathData::new()
        .move_to(ScreenPoint::new(bottom.x - radius_x, top.y))
        .line_to(ScreenPoint::new(bottom.x - radius_x, bottom.y))
        .arc_to(radius_x, radius_y, false, false, ScreenPoint::new(bottom.x + radius_x, bottom.y))
        .line_to(ScreenPoint::new(bottom.x + radius_x, top.y))
        .close()
        .include(extent);
    let side_style = Style { fill: Paint::Reference(gradient_id), ..Style::default() }.with_stroke(outline(theme));
    canvas.path(&side, &side_style);
    canvas.ellipse(top, radius_x, radius_y, &outlined(theme, color.shade(CYLINDER_TOP_SHADE)));
}

/// Soft blurred ellipse under a node. Registers the `blur` filter def.
pub fn shadow(canvas: &mut IsoCanvas, theme: &Theme, center: PlanPoint, radius: f64) {
    let filter = format!(
        "<filter id=\"blur\" x=\"-50%\" y=\"-50%\" width=\"200%\" height=\"200%\"><feGaussianBlur stdDeviation=\"{}\"/></filter>",
        theme.shadow.blur
    );
    canvas.define("blur", filter);
    let position = canvas.project_quiet(PlanPoint::new(center.x + SHADOW_OFFSET.0, center.y + SHADOW_OFFSET.1), 0.0);
    let (radius_x, radius_y) = canvas.projection().circle_radii(radius);
    let style = Style::fill(theme.shadow.color).with_opacity(theme.shadow.opacity).with_filter("blur");
    canvas.ellipse(position, radius_x, radius_y, &style);
}

/// Icon-local coordinate frame: `(a, b)` in tiles around the node center, `z` in tiles above its base.
/// The local `+b` face is the icon's front. See the module docs for the quarter-turn swap.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IconFrame {
    /// Center on the plan (rotated world).
    pub center: PlanPoint,
    /// Base elevation (tiles).
    pub z: f64,
    /// Scale of the icon (`size`).
    pub scale: f64,
    /// Odd camera rotation: local `a` runs along plan `y`.
    pub swap: bool,
    /// Tile size (px), for pixel-sized details.
    pub tile: f64,
}

impl IconFrame {
    /// Frame of a node drawn at `center`/`z` with `scale`, for a camera `rotation` and tile size.
    pub fn new(center: PlanPoint, z: f64, scale: f64, rotation: Rotation, tile: f64) -> Self {
        Self { center, z, scale, swap: rotation.quarter_turns() % 2 == 1, tile }
    }

    /// Local `(a, b)` → plan point.
    pub fn plan(&self, a: f64, b: f64) -> PlanPoint {
        let (x, y) = if self.swap { (b, a) } else { (a, b) };
        PlanPoint::new(self.center.x + x * self.scale, self.center.y + y * self.scale)
    }

    /// Local height → elevation.
    pub fn elevation(&self, z: f64) -> f64 {
        self.z + z * self.scale
    }

    /// Local length → plan length.
    pub fn tiles(&self, length: f64) -> f64 {
        length * self.scale
    }

    /// A detail size designed in px at the default tile, scaled with the icon and the tile.
    pub fn px(&self, pixels: f64) -> f64 {
        pixels * self.scale * self.tile / DEFAULT_TILE_PX
    }

    /// Projects (and tracks) a local point.
    pub fn point(&self, canvas: &mut IsoCanvas, a: f64, b: f64, z: f64) -> ScreenPoint {
        canvas.project(self.plan(a, b), self.elevation(z))
    }

    /// Shade factor of the local front (`+b`) face: plan `+y` (left) or, swapped, plan `+x` (right).
    pub fn front_shade(&self, theme: &Theme) -> f64 {
        if self.swap { theme.faces.right } else { theme.faces.left }
    }

    /// Shade factor of the local side (`+a`) face.
    pub fn side_shade(&self, theme: &Theme) -> f64 {
        if self.swap { theme.faces.left } else { theme.faces.right }
    }
}

/// An axis-aligned box in icon-local coordinates (min corner + size, tiles).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LocalBox {
    /// Min `a`.
    pub a: f64,
    /// Min `b`.
    pub b: f64,
    /// Base height.
    pub z: f64,
    /// Extent along `a`.
    pub width: f64,
    /// Extent along `b`.
    pub depth: f64,
    /// Height.
    pub height: f64,
}

impl LocalBox {
    /// Box from its min corner and size.
    pub const fn new(a: f64, b: f64, z: f64, width: f64, depth: f64, height: f64) -> Self {
        Self { a, b, z, width, depth, height }
    }

    /// Box centered on the node, `half_width × half_depth`, from `z` up by `height`.
    pub const fn centered(half_width: f64, half_depth: f64, z: f64, height: f64) -> Self {
        Self::new(-half_width, -half_depth, z, 2.0 * half_width, 2.0 * half_depth, height)
    }

    /// Top height.
    pub fn top(&self) -> f64 {
        self.z + self.height
    }
}

/// A visible face of a [`LocalBox`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Face {
    /// Local `+b` face (the icon's front).
    Front,
    /// Local `+a` face.
    Side,
    /// Top face.
    Top,
}

/// A rectangle on a face, addressed with `u` (screen left → right) and `v` (bottom → top, or back → front
/// on the top face), both in `0..=1`. Mirroring on odd rotations is handled here, so glyphs never flip.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceRegion {
    solid: LocalBox,
    face: Face,
    u_range: (f64, f64),
    v_range: (f64, f64),
}

impl FaceRegion {
    /// The whole face.
    pub fn of(solid: LocalBox, face: Face) -> Self {
        Self { solid, face, u_range: (0.0, 1.0), v_range: (0.0, 1.0) }
    }

    /// Sub-rectangle `[u0, u1] × [v0, v1]` of this region.
    pub fn inset(&self, u0: f64, u1: f64, v0: f64, v1: f64) -> Self {
        let lerp = |range: (f64, f64), t: f64| range.0 + (range.1 - range.0) * t;
        Self {
            u_range: (lerp(self.u_range, u0), lerp(self.u_range, u1)),
            v_range: (lerp(self.v_range, v0), lerp(self.v_range, v1)),
            ..*self
        }
    }

    /// Local `(a, b, z)` of a region point.
    pub fn local(&self, frame: &IconFrame, u: f64, v: f64) -> (f64, f64, f64) {
        let u = self.u_range.0 + (self.u_range.1 - self.u_range.0) * u;
        let v = self.v_range.0 + (self.v_range.1 - self.v_range.0) * v;
        let solid = &self.solid;
        let vertical = solid.z + solid.height * v;
        match self.face {
            // Screen-left is −x along a +y face, and +y along a +x face.
            Face::Front => {
                let along = if frame.swap { 1.0 - u } else { u };
                (solid.a + solid.width * along, solid.b + solid.depth, vertical)
            }
            Face::Side => {
                let along = if frame.swap { u } else { 1.0 - u };
                (solid.a + solid.width, solid.b + solid.depth * along, vertical)
            }
            Face::Top => (solid.a + solid.width * u, solid.b + solid.depth * v, solid.top()),
        }
    }

    /// Projects (and tracks) a region point.
    pub fn point(&self, canvas: &mut IsoCanvas, frame: &IconFrame, u: f64, v: f64) -> ScreenPoint {
        let (a, b, z) = self.local(frame, u, v);
        frame.point(canvas, a, b, z)
    }

    /// Polygon through region points.
    pub fn polygon(&self, canvas: &mut IsoCanvas, frame: &IconFrame, points: &[(f64, f64)], style: &Style) {
        let screen: Vec<ScreenPoint> = points.iter().map(|&(u, v)| self.point(canvas, frame, u, v)).collect();
        canvas.polygon(&screen, style);
    }

    /// Filled sub-rectangle.
    pub fn fill_rect(&self, canvas: &mut IsoCanvas, frame: &IconFrame, u: (f64, f64), v: (f64, f64), style: &Style) {
        self.polygon(canvas, frame, &[(u.0, v.0), (u.1, v.0), (u.1, v.1), (u.0, v.1)], style);
    }

    /// Open polyline through region points.
    pub fn polyline(&self, canvas: &mut IsoCanvas, frame: &IconFrame, points: &[(f64, f64)], stroke: &Stroke) {
        let mut path = PathData::new();
        for (index, &(u, v)) in points.iter().enumerate() {
            let point = self.point(canvas, frame, u, v);
            path = if index == 0 { path.move_to(point) } else { path.line_to(point) };
        }
        canvas.path(&path, &Style::stroke(stroke.clone()));
    }

    /// Round dot (LED, port) of `radius_px` (designed at the default tile).
    pub fn dot(&self, canvas: &mut IsoCanvas, frame: &IconFrame, at: (f64, f64), radius_px: f64, color: Color) {
        let center = self.point(canvas, frame, at.0, at.1);
        canvas.circle(center, frame.px(radius_px), &Style::fill(color));
    }
}

/// A box in local coordinates (see [`iso_box`]).
pub fn solid_box(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, solid: &LocalBox, color: Color) {
    let corner = frame.plan(solid.a, solid.b);
    let opposite = frame.plan(solid.a + solid.width, solid.b + solid.depth);
    let origin = PlanPoint::new(corner.x.min(opposite.x), corner.y.min(opposite.y));
    let size = [(opposite.x - corner.x).abs(), (opposite.y - corner.y).abs(), frame.tiles(solid.height)];
    iso_box(canvas, theme, origin, size, frame.elevation(solid.z), color);
}

/// A round solid standing on the floor of the frame: center, radius, base height and height (tiles).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoundSolid {
    /// Center `(a, b)`.
    pub at: (f64, f64),
    /// Radius.
    pub radius: f64,
    /// Base height.
    pub z: f64,
    /// Height (cylinder) or rise (dome).
    pub height: f64,
}

/// A vertical cylinder in local coordinates (see [`cylinder`]).
pub fn solid_cylinder(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, solid: &RoundSolid, color: Color) {
    let center = frame.plan(solid.at.0, solid.at.1);
    let (radius, height) = (frame.tiles(solid.radius), frame.tiles(solid.height));
    cylinder(canvas, theme, center, radius, height, frame.elevation(solid.z), color);
}

/// The node shadow, skipped when the theme has none (`mono`).
pub fn node_shadow(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, radius: f64) {
    if theme.has_shadow() {
        shadow(canvas, theme, frame.center, frame.tiles(radius));
    }
}

/// Style of the ports drawn along a slab's front.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PortStyle {
    /// Light round dots (load balancer, as in the prototype).
    Dots,
    /// Dark rectangular sockets (switch).
    Sockets,
}

/// Row of ports along the front face of a slab.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PortRow {
    /// Number of ports.
    pub count: usize,
    /// Port color.
    pub color: Color,
    /// Look.
    pub style: PortStyle,
    /// Horizontal span on the face `(u0, u1)`.
    pub span: (f64, f64),
}

/// `slab`: a low box with a row of ports on its front face. Returns the `u` of each port on that face.
pub fn slab(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    frame: &IconFrame,
    solid: &LocalBox,
    color: Color,
    ports: &PortRow,
) -> Vec<f64> {
    solid_box(canvas, theme, frame, solid, color);
    let front = FaceRegion::of(*solid, Face::Front);
    let steps = ports.count.saturating_sub(1).max(1) as f64;
    let positions: Vec<f64> =
        (0..ports.count).map(|index| ports.span.0 + (ports.span.1 - ports.span.0) * index as f64 / steps).collect();
    for &u in &positions {
        match ports.style {
            PortStyle::Dots => front.dot(canvas, frame, (u, 0.5), 2.2, ports.color),
            PortStyle::Sockets => {
                let half = (ports.span.1 - ports.span.0) / steps * 0.32;
                front.fill_rect(canvas, frame, (u - half, u + half), (0.28, 0.72), &Style::fill(ports.color));
            }
        }
    }
    positions
}

/// `stack`: `count` boxes of `unit` size piled `pitch` apart, each with an LED bar and a status dot on
/// its front face (server, the prototype's look). With `animate`, the LED bars blink in sequence (T-402).
pub fn stack(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    frame: &IconFrame,
    unit: &LocalBox,
    (count, pitch): (usize, f64),
    color: Color,
    animate: bool,
) {
    let led = Stroke::new(theme.nodes.led, frame.px(2.2)).round_cap();
    for index in 0..count {
        let layer = LocalBox { z: unit.z + pitch * index as f64, ..*unit };
        solid_box(canvas, theme, frame, &layer, color);
        let front = FaceRegion::of(layer, Face::Front);
        // Top unit first: the blink runs down the stack.
        let blink = IconMotion::Blink { phase: (count - 1 - index) as f64 / count as f64 };
        if animate {
            open_hook(canvas, &blink);
        }
        front.polyline(canvas, frame, &[(0.14, 0.5), (0.56, 0.5)], &led);
        if animate {
            close_hook(canvas);
        }
        front.dot(canvas, frame, (0.84, 0.5), 1.7, theme.nodes.accent);
    }
}

/// Convex hull (Andrew's monotone chain), counter-clockwise on screen, deterministic.
fn convex_hull(mut points: Vec<ScreenPoint>) -> Vec<ScreenPoint> {
    points.sort_by(|p, q| p.x.total_cmp(&q.x).then(p.y.total_cmp(&q.y)));
    let cross = |o: ScreenPoint, a: ScreenPoint, b: ScreenPoint| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut hull: Vec<ScreenPoint> = Vec::with_capacity(points.len() + 1);
    for pass in 0..2 {
        let start = hull.len();
        let ordered: Box<dyn Iterator<Item = &ScreenPoint>> =
            if pass == 0 { Box::new(points.iter()) } else { Box::new(points.iter().rev()) };
        for &point in ordered {
            while hull.len() >= start + 2 && cross(hull[hull.len() - 2], hull[hull.len() - 1], point) <= 0.0 {
                hull.pop();
            }
            hull.push(point);
        }
        hull.pop();
    }
    hull
}

/// Cross-section circle of a lying tube at local `a`, sampled over angles `[from, to]` (radians, 0 = +b).
fn tube_section(
    frame: &IconFrame,
    canvas: &IsoCanvas,
    a: f64,
    axis: (f64, f64),
    radius: f64,
    angles: (f64, f64),
) -> Vec<ScreenPoint> {
    (0..=CIRCLE_SAMPLES)
        .map(|step| {
            let angle = angles.0 + (angles.1 - angles.0) * step as f64 / CIRCLE_SAMPLES as f64;
            let b = axis.0 + radius * angle.cos();
            let z = axis.1 + radius * angle.sin();
            canvas.project_quiet(frame.plan(a, b), frame.elevation(z))
        })
        .collect()
}

/// A segment of a lying tube along local `a`, from `a0` to `a1` (towards the camera), axis at `(b, z)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TubeSegment {
    /// Start along `a`.
    pub a0: f64,
    /// End along `a` (the visible cap).
    pub a1: f64,
    /// Axis position `(b, z)`.
    pub axis: (f64, f64),
    /// Radius.
    pub radius: f64,
}

/// `tube`: one lying cylinder segment (body hull, a lit top band and the near cap).
pub fn tube(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, segment: &TubeSegment, color: Color) {
    let full = (0.0, std::f64::consts::TAU);
    let mut sections = tube_section(frame, canvas, segment.a0, segment.axis, segment.radius, full);
    sections.extend(tube_section(frame, canvas, segment.a1, segment.axis, segment.radius, full));
    let body = convex_hull(sections);
    canvas.polygon(&body, &outlined(theme, color.shade(theme.faces.left)));
    // Upper quarter of the surface faces the light: a lighter band, no outline.
    let band_angles = (std::f64::consts::FRAC_PI_4, 0.75 * std::f64::consts::PI);
    let mut band = tube_section(frame, canvas, segment.a0, segment.axis, segment.radius * 0.97, band_angles);
    band.extend(tube_section(frame, canvas, segment.a1, segment.axis, segment.radius * 0.97, band_angles));
    canvas.polygon(&convex_hull(band), &Style::fill(color.shade(theme.faces.top)));
    let cap = tube_section(frame, canvas, segment.a1, segment.axis, segment.radius, full);
    canvas.polygon(&cap, &outlined(theme, color.shade(frame.side_shade(theme))));
}

/// `dome`: a spheroidal cap rising `solid.height` tiles over a circle of `solid.radius`.
pub fn dome(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, solid: &RoundSolid, color: Color) {
    let RoundSolid { at, radius, z, height: rise } = *solid;
    let base = canvas.project_quiet(frame.plan(at.0, at.1), frame.elevation(z));
    let (radius_x, radius_y) = canvas.projection().circle_radii(frame.tiles(radius));
    // Silhouette of a spheroid: the base ellipse's vertical radius combined with the projected rise.
    let rise_px = frame.tiles(rise) * canvas.tile();
    let radius_top = (radius_y * radius_y + rise_px * rise_px).sqrt();
    let left = ScreenPoint::new(base.x - radius_x, base.y);
    let right = ScreenPoint::new(base.x + radius_x, base.y);
    let shape = PathData::new()
        .move_to(left)
        .arc_to(radius_x, radius_top, false, true, right)
        .arc_to(radius_x, radius_y, false, true, left)
        .close()
        .include(Rect::new(left.x, base.y - radius_top, right.x, base.y + radius_y));
    let gradient = sphere_gradient(canvas, color.shade(CYLINDER_TOP_SHADE), color.shade(0.72));
    canvas.path(&shape, &Style { fill: Paint::Reference(gradient), ..Style::default() }.with_stroke(outline(theme)));
}

/// `sphere`: a ball of `radius` tiles centered at local `(a, b, z)`; returns its screen center and radius.
pub fn sphere(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    frame: &IconFrame,
    center: (f64, f64, f64),
    radius: f64,
    color: Color,
) -> (ScreenPoint, f64) {
    let screen = canvas.project_quiet(frame.plan(center.0, center.1), frame.elevation(center.2));
    // A sphere projects to a circle as wide as its equator ellipse.
    let (radius_px, _) = canvas.projection().circle_radii(frame.tiles(radius));
    let gradient = sphere_gradient(canvas, color.shade(1.18), color.shade(0.7));
    canvas.circle(
        screen,
        radius_px,
        &Style { fill: Paint::Reference(gradient), ..Style::default() }.with_stroke(outline(theme)),
    );
    (screen, radius_px)
}

/// `cloud`: overlapping puffs with a single unified outline (all outlines first, fills on top).
pub fn cloud(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, light: Color, shade: Color) {
    let gradient_id = format!("cloud{}{}", &light.to_hex()[1..], &shade.to_hex()[1..]);
    canvas.define(
        gradient_id.clone(),
        format!(
            "<radialGradient id=\"{gradient_id}\" cx=\"0.35\" cy=\"0.3\" r=\"0.8\"><stop offset=\"0\" stop-color=\"{light}\"/><stop offset=\"1\" stop-color=\"{shade}\"/></radialGradient>"
        ),
    );
    let center = canvas.project_quiet(frame.plan(0.0, 0.0), frame.elevation(CLOUD_CENTER_Z));
    let puff = |(dx, dy, radius): (f64, f64, f64)| {
        (ScreenPoint::new(center.x + frame.px(dx), center.y + frame.px(dy)), frame.px(radius))
    };
    // The outer half of a doubled stroke is all that stays visible once the fills cover the inner half.
    let rim = Style::stroke(Stroke::new(theme.stroke, 2.0 * theme.stroke_width));
    for each in CLOUD_PUFFS {
        let (position, radius) = puff(each);
        canvas.circle(position, radius, &rim);
    }
    let fill = Style { fill: Paint::Reference(gradient_id), ..Style::default() };
    for each in CLOUD_PUFFS {
        let (position, radius) = puff(each);
        canvas.circle(position, radius, &fill);
    }
}

/// Points sampled on each puff when outlining the cloud.
const CLOUD_OUTLINE_SAMPLES: usize = 16;

/// Screen outline (convex hull) of the cloud's puffs as [`cloud`] draws them: the body floats above its
/// footprint, so the prism's silhouette reaches the floor where nothing is drawn (T-425: rails stopped short).
pub(crate) fn cloud_outline(frame: &IconFrame, projection: &Projection) -> Vec<ScreenPoint> {
    let center = projection.project(frame.plan(0.0, 0.0), frame.elevation(CLOUD_CENTER_Z));
    let points = CLOUD_PUFFS
        .iter()
        .flat_map(|&(dx, dy, radius)| {
            let (x, y, radius) = (center.x + frame.px(dx), center.y + frame.px(dy), frame.px(radius));
            (0..CLOUD_OUTLINE_SAMPLES).map(move |index| {
                let angle = std::f64::consts::TAU * index as f64 / CLOUD_OUTLINE_SAMPLES as f64;
                ScreenPoint::new(x + radius * angle.cos(), y + radius * angle.sin())
            })
        })
        .collect();
    convex_hull(points)
}

/// Body proportions of a [`person`] (tiles).
const PERSON_BODY_RADIUS: f64 = 0.16;
const PERSON_BODY_HEIGHT: f64 = 0.32;
const PERSON_HEAD_RADIUS: f64 = 0.13;
const PERSON_NECK: f64 = 0.04;
/// Top of a [`person`] (tiles): body, shoulders (hemisphere), neck and head.
pub const PERSON_HEIGHT: f64 = PERSON_BODY_HEIGHT + PERSON_BODY_RADIUS + PERSON_NECK + 2.0 * PERSON_HEAD_RADIUS;

/// `person`: a capsule body (cylinder + round shoulders) and a spherical head, standing at local `at`.
pub fn person(canvas: &mut IsoCanvas, theme: &Theme, frame: &IconFrame, at: (f64, f64), body: Color, head: Color) {
    let bottom = canvas.project_quiet(frame.plan(at.0, at.1), frame.elevation(0.0));
    let top = canvas.project_quiet(frame.plan(at.0, at.1), frame.elevation(PERSON_BODY_HEIGHT));
    let (radius_x, radius_y) = canvas.projection().circle_radii(frame.tiles(PERSON_BODY_RADIUS));
    let capsule = PathData::new()
        .move_to(ScreenPoint::new(bottom.x - radius_x, top.y))
        .line_to(ScreenPoint::new(bottom.x - radius_x, bottom.y))
        .arc_to(radius_x, radius_y, false, false, ScreenPoint::new(bottom.x + radius_x, bottom.y))
        .line_to(ScreenPoint::new(bottom.x + radius_x, top.y))
        .arc_to(radius_x, radius_x, false, false, ScreenPoint::new(bottom.x - radius_x, top.y))
        .close()
        .include(Rect::new(bottom.x - radius_x, top.y - radius_x, bottom.x + radius_x, bottom.y + radius_y));
    let gradient = side_gradient(canvas, body);
    canvas.path(&capsule, &Style { fill: Paint::Reference(gradient), ..Style::default() }.with_stroke(outline(theme)));
    let head_z = PERSON_BODY_HEIGHT + PERSON_BODY_RADIUS + PERSON_NECK + PERSON_HEAD_RADIUS;
    sphere(canvas, theme, frame, (at.0, at.1, head_z), PERSON_HEAD_RADIUS, head);
}

/// `screen`: a panel box with a glass on its front face, inset by `bezel` (fraction of the face).
/// Returns the glass region, for the caller's content.
pub fn screen(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    frame: &IconFrame,
    panel: &LocalBox,
    body: Color,
    glass: Color,
    bezel: (f64, f64),
) -> FaceRegion {
    solid_box(canvas, theme, frame, panel, body);
    let region = FaceRegion::of(*panel, Face::Front).inset(bezel.0, 1.0 - bezel.0, bezel.1, 1.0 - bezel.1);
    let shade = frame.front_shade(theme);
    region.fill_rect(canvas, frame, (0.0, 1.0), (0.0, 1.0), &outlined(theme, glass.shade(shade.max(0.9))));
    if theme.icon_finish == IconFinish::Silver {
        glass_reflection(canvas, frame, &region, theme.nodes.paper);
    }
    region
}

/// Glare bands across a glass (silver finish): a wide faint one and a thin bright one, slanted like
/// light from the top-left window of the room.
fn glass_reflection(canvas: &mut IsoCanvas, frame: &IconFrame, glass: &FaceRegion, light: Color) {
    for &(band, opacity) in &GLASS_GLARE {
        let (left, right) = band;
        let points = [(left, 1.0), (right, 1.0), (right - GLARE_SLANT, 0.0), (left - GLARE_SLANT, 0.0)];
        let clipped: Vec<(f64, f64)> = points.iter().map(|&(u, v)| (u.clamp(0.0, 1.0), v)).collect();
        glass.polygon(canvas, frame, &clipped, &Style::fill(light).with_opacity(opacity));
    }
}

/// An affine parallelogram on screen: `origin + along·u + up·v` (prism faces).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ScreenQuad {
    /// Bottom-left corner.
    pub origin: ScreenPoint,
    /// Bottom edge, left → right.
    pub along: ScreenVector,
    /// Left edge, bottom → top.
    pub up: ScreenVector,
}

impl ScreenQuad {
    /// Point at `(u, v)`.
    pub fn point(&self, u: f64, v: f64) -> ScreenPoint {
        ScreenPoint::new(
            self.origin.x + self.along.x * u + self.up.x * v,
            self.origin.y + self.along.y * u + self.up.y * v,
        )
    }

    /// Open polyline through quad points.
    pub fn polyline(&self, canvas: &mut IsoCanvas, points: &[(f64, f64)], stroke: &Stroke, closed: bool) {
        let mut path = PathData::new();
        for (index, &(u, v)) in points.iter().enumerate() {
            let point = self.point(u, v);
            path = if index == 0 { path.move_to(point) } else { path.line_to(point) };
        }
        if closed {
            path = path.close();
        }
        canvas.path(&path, &Style::stroke(stroke.clone()));
    }
}

/// A regular prism (hexagon, heptagon…) in local coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Prism {
    /// Number of sides.
    pub sides: usize,
    /// Circumradius (tiles).
    pub radius: f64,
    /// Base height.
    pub z: f64,
    /// Height.
    pub height: f64,
    /// Angle of the first vertex (degrees, from local `+a` towards `+b`).
    pub phase_degrees: f64,
}

/// Shade of a vertical face from the plan angle of its outward normal (0° = +x, 90° = +y), interpolating
/// the theme's right/left shading and extrapolating a little for faces turned further away.
fn vertical_face_shade(theme: &Theme, normal_degrees: f64) -> f64 {
    let shade = theme.faces.right + (theme.faces.left - theme.faces.right) * normal_degrees / 90.0;
    shade.clamp(theme.faces.right * PRISM_SHADE_SPREAD.0, theme.faces.left * PRISM_SHADE_SPREAD.1)
}

/// `prism`: visible side faces (shaded by orientation) and the top. Returns the face whose normal points
/// most directly at the camera, for glyphs.
pub fn prism(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    frame: &IconFrame,
    shape: &Prism,
    color: Color,
) -> Option<ScreenQuad> {
    let corners: Vec<PlanPoint> = (0..shape.sides)
        .map(|index| {
            let angle = (shape.phase_degrees + 360.0 * index as f64 / shape.sides as f64).to_radians();
            frame.plan(shape.radius * angle.cos(), shape.radius * angle.sin())
        })
        .collect();
    let (bottom, top) = (frame.elevation(shape.z), frame.elevation(shape.z + shape.height));
    let mut faces: Vec<(f64, f64, PlanPoint, PlanPoint)> = Vec::new();
    for index in 0..shape.sides {
        let (start, end) = (corners[index], corners[(index + 1) % shape.sides]);
        let middle = PlanPoint::new((start.x + end.x) / 2.0, (start.y + end.y) / 2.0);
        let normal = (middle.x - frame.center.x, middle.y - frame.center.y);
        // Faces looking towards +x+y are the ones the camera sees.
        let facing = normal.0 + normal.1;
        if facing > 1e-9 {
            faces.push((middle.x + middle.y, normal.1.atan2(normal.0).to_degrees(), start, end));
        }
    }
    faces.sort_by(|p, q| p.0.total_cmp(&q.0));
    let mut front: Option<(f64, ScreenQuad)> = None;
    for &(_, normal_degrees, start, end) in &faces {
        let quad = [
            canvas.project(start, bottom),
            canvas.project(end, bottom),
            canvas.project(end, top),
            canvas.project(start, top),
        ];
        canvas.polygon(&quad, &outlined(theme, color.shade(vertical_face_shade(theme, normal_degrees))));
        let (left, right) = if quad[0].x <= quad[1].x { (quad[0], quad[1]) } else { (quad[1], quad[0]) };
        let lifted = canvas.project_quiet(start, top);
        let up = ScreenVector::new(0.0, lifted.y - canvas.project_quiet(start, bottom).y);
        let candidate = ScreenQuad { origin: left, along: left.to(right), up };
        let directness = -(normal_degrees - 45.0).abs();
        if front.is_none_or(|(best, _)| directness > best) {
            front = Some((directness, candidate));
        }
    }
    canvas.plan_polygon(&corners, top, &outlined(theme, color.shade(theme.faces.top)));
    front.map(|(_, quad)| quad)
}

/// A vertical pentagon-shaped sign plate (arrow) along local `a` (`along_a`) or local `b`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SignPlate {
    /// The plate runs along local `a` (visible on the front face) or along local `b` (visible on the side).
    pub along_a: bool,
    /// Offset across the plate (the other local axis).
    pub across: f64,
    /// Start and tip along the plate.
    pub span: (f64, f64),
    /// Bottom and top heights.
    pub heights: (f64, f64),
    /// Thickness.
    pub thickness: f64,
}

/// Draws a sign plate with an arrow tip; returns the screen quad of its face for marks.
pub fn sign_plate(
    canvas: &mut IsoCanvas,
    theme: &Theme,
    frame: &IconFrame,
    plate: &SignPlate,
    color: Color,
) -> ScreenQuad {
    let tip_length = (plate.span.1 - plate.span.0) * 0.2;
    let body_end = plate.span.1 - tip_length;
    let (bottom, top) = plate.heights;
    let middle = (bottom + top) / 2.0;
    let half = plate.thickness / 2.0;
    let local = |along: f64, across: f64| if plate.along_a { (along, across) } else { (across, along) };
    let mut project = |along: f64, across: f64, z: f64| {
        let (a, b) = local(along, across);
        frame.point(canvas, a, b, z)
    };
    let face_offset = plate.across + half;
    let top_strip = [
        project(plate.span.0, plate.across - half, top),
        project(body_end, plate.across - half, top),
        project(body_end, face_offset, top),
        project(plate.span.0, face_offset, top),
    ];
    let slant = [
        project(body_end, plate.across - half, top),
        project(plate.span.1, plate.across - half, middle),
        project(plate.span.1, face_offset, middle),
        project(body_end, face_offset, top),
    ];
    let face = [
        project(plate.span.0, face_offset, bottom),
        project(body_end, face_offset, bottom),
        project(plate.span.1, face_offset, middle),
        project(body_end, face_offset, top),
        project(plate.span.0, face_offset, top),
    ];
    canvas.polygon(&top_strip, &outlined(theme, color.shade(theme.faces.top)));
    canvas.polygon(&slant, &outlined(theme, color.shade(SLANTED_TOP_SHADE)));
    let shade = if plate.along_a { frame.front_shade(theme) } else { frame.side_shade(theme) };
    canvas.polygon(&face, &outlined(theme, color.shade(shade)));
    let (start, end) = if face[0].x <= face[1].x { (face[0], face[1]) } else { (face[1], face[0]) };
    ScreenQuad { origin: start, along: start.to(end), up: face[0].to(face[4]) }
}

/// Plan-flat arrow (shaft + head) lying on a horizontal plane at local height `z`, from `from` to `to`.
pub fn floor_arrow(
    canvas: &mut IsoCanvas,
    frame: &IconFrame,
    from: (f64, f64),
    to: (f64, f64),
    z: f64,
    widths: (f64, f64),
    style: &Style,
) {
    let (dx, dy) = (to.0 - from.0, to.1 - from.1);
    let length = (dx * dx + dy * dy).sqrt().max(1e-9);
    let (ux, uy) = (dx / length, dy / length);
    let (nx, ny) = (-uy, ux);
    let (shaft, head) = (widths.0 / 2.0, widths.1 / 2.0);
    let head_length = widths.1 * 0.9;
    let neck = (to.0 - ux * head_length, to.1 - uy * head_length);
    let outline_points = [
        (from.0 + nx * shaft, from.1 + ny * shaft),
        (neck.0 + nx * shaft, neck.1 + ny * shaft),
        (neck.0 + nx * head, neck.1 + ny * head),
        to,
        (neck.0 - nx * head, neck.1 - ny * head),
        (neck.0 - nx * shaft, neck.1 - ny * shaft),
        (from.0 - nx * shaft, from.1 - ny * shaft),
    ];
    let screen: Vec<ScreenPoint> = outline_points.iter().map(|&(a, b)| frame.point(canvas, a, b, z)).collect();
    canvas.polygon(&screen, style);
}

/// Projected plan circle (ring) at local height `z`, as a stroked ellipse.
pub fn floor_ring(canvas: &mut IsoCanvas, frame: &IconFrame, at: (f64, f64), radius: f64, z: f64, stroke: &Stroke) {
    let center = frame.point(canvas, at.0, at.1, z);
    let (radius_x, radius_y) = canvas.projection().circle_radii(frame.tiles(radius));
    canvas.ellipse(center, radius_x, radius_y, &Style::stroke(stroke.clone()));
}

/// A rounded pill with centered bold text (replica `×N` badge, `badge:` labels), centered at `center`.
pub fn badge(canvas: &mut IsoCanvas, theme: &Theme, center: ScreenPoint, text: &str) -> Rect {
    let width = text.chars().count() as f64 * BADGE_FONT_SIZE * BADGE_GLYPH_WIDTH + 2.0 * BADGE_PADDING;
    let rect = Rect::centered(center, width.max(BADGE_HEIGHT), BADGE_HEIGHT);
    let style = Style::fill(theme.pill.background).with_stroke(Stroke::new(theme.background, 1.5));
    canvas.rect(rect, BADGE_HEIGHT / 2.0, &style);
    let text_style = TextStyle {
        size: BADGE_FONT_SIZE,
        weight: 700,
        anchor: TextAnchor::Middle,
        fill: theme.pill.text,
        letter_spacing: 0.0,
    };
    // Baseline so that the cap height sits centered in the pill.
    canvas.text(ScreenPoint::new(center.x, center.y + BADGE_FONT_SIZE * 0.36), text, &text_style);
    rect
}

/// Stroke for glyphs and details, round caps and joins.
pub fn detail_stroke(frame: &IconFrame, color: Color, width_px: f64) -> Stroke {
    let mut stroke = Stroke::new(color, frame.px(width_px));
    stroke.cap = LineCap::Round;
    stroke
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Projection;

    fn frame(rotation: Rotation) -> IconFrame {
        IconFrame::new(PlanPoint::new(2.0, 3.0), 0.0, 1.0, rotation, DEFAULT_TILE_PX)
    }

    #[test]
    fn the_cloud_outline_floats_above_its_floor_point() {
        let projection = Projection::new(DEFAULT_TILE_PX);
        let frame = frame(Rotation::R0);
        let outline = cloud_outline(&frame, &projection);
        let floor = projection.project(frame.plan(0.0, 0.0), 0.0);
        let bottom = outline.iter().map(|point| point.y).fold(f64::NEG_INFINITY, f64::max);
        assert!(outline.len() >= 3);
        assert!(
            (bottom - floor.y).abs() < 6.0,
            "the puffs end about where the floor point is: {bottom} vs {}",
            floor.y
        );
        let top = outline.iter().map(|point| point.y).fold(f64::INFINITY, f64::min);
        assert!(top < floor.y - frame.px(30.0), "and stand well above it: {top}");
    }

    #[test]
    fn swap_reflects_local_axes() {
        assert_eq!(frame(Rotation::R0).plan(0.5, 0.25), PlanPoint::new(2.5, 3.25));
        assert_eq!(frame(Rotation::R90).plan(0.5, 0.25), PlanPoint::new(2.25, 3.5));
        assert!(!frame(Rotation::R180).swap && frame(Rotation::R270).swap);
    }

    #[test]
    fn front_face_runs_left_to_right_on_screen_in_every_rotation() {
        let solid = LocalBox::centered(0.5, 0.3, 0.0, 0.4);
        for rotation in Rotation::ALL {
            let frame = frame(rotation);
            let mut canvas = IsoCanvas::new(Projection::default());
            for face in [Face::Front, Face::Side] {
                let region = FaceRegion::of(solid, face);
                let left = region.point(&mut canvas, &frame, 0.0, 0.5);
                let right = region.point(&mut canvas, &frame, 1.0, 0.5);
                assert!(left.x < right.x, "{rotation:?} {face:?}");
            }
        }
    }

    #[test]
    fn hull_of_square_keeps_corners() {
        let points = vec![
            ScreenPoint::new(0.0, 0.0),
            ScreenPoint::new(1.0, 0.0),
            ScreenPoint::new(0.5, 0.5),
            ScreenPoint::new(1.0, 1.0),
            ScreenPoint::new(0.0, 1.0),
        ];
        assert_eq!(convex_hull(points).len(), 4);
    }

    #[test]
    fn prism_front_face_points_at_the_camera() {
        let mut canvas = IsoCanvas::new(Projection::default());
        let theme = Theme::blueprint();
        let shape = Prism { sides: 6, radius: 0.4, z: 0.0, height: 0.5, phase_degrees: 15.0 };
        let quad = prism(&mut canvas, &theme, &frame(Rotation::R0), &shape, theme.nodes.function);
        let quad = quad.map(|quad| (quad.along.x, quad.along.y.abs(), quad.up.y));
        // The face looking at 45° projects to a horizontal edge.
        let (along_x, along_y, up_y) = quad.unwrap_or_default();
        assert!(along_x > 0.0 && along_y < 1e-6 && up_y < 0.0);
    }
}
