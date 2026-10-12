//! `IsoCanvas`: a tiny SVG writer that projects plan points and tracks screen bounding boxes,
//! globally and per item (node, label, edge…). Icons and the scene draw only through it, so
//! every drawn pixel is accounted for in collision boxes and in the auto-fit `viewBox`.
//!
//! Output is deterministic: numbers use [`fmt_num`] (1 decimal, no trailing `.0`), defs are sorted by id.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::color::Color;
use crate::geometry::{Affine, PlanPoint, Projection, Rect, ScreenPoint};

/// Formats a coordinate with fixed precision (1 decimal), trimming `.0` and normalizing `-0`.
pub fn fmt_num(value: f64) -> String {
    let rounded = (value * 10.0).round() / 10.0;
    let rounded = if rounded == 0.0 { 0.0 } else { rounded };
    if rounded.fract() == 0.0 { format!("{rounded:.0}") } else { format!("{rounded:.1}") }
}

/// Formats a ratio/opacity/matrix coefficient with up to 4 decimals, trimmed.
pub fn fmt_ratio(value: f64) -> String {
    let text = format!("{value:.4}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() { "0".to_owned() } else { text.to_owned() }
}

/// `x,y x,y …` for `points="…"`.
pub fn fmt_points(points: &[ScreenPoint]) -> String {
    points.iter().map(|point| format!("{},{}", fmt_num(point.x), fmt_num(point.y))).collect::<Vec<_>>().join(" ")
}

/// SVG `matrix(a,b,c,d,e,f)`.
pub fn fmt_matrix(affine: &Affine) -> String {
    format!(
        "matrix({},{},{},{},{},{})",
        fmt_ratio(affine.a),
        fmt_ratio(affine.b),
        fmt_ratio(affine.c),
        fmt_ratio(affine.d),
        fmt_num(affine.e),
        fmt_num(affine.f)
    )
}

/// Escapes `& < > " '` for text content and attribute values.
pub fn escape_xml(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        match character {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

/// Semantic role of a drawn item; emitted as `data-role` for desktop hit-testing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ItemRole {
    /// Background grid.
    Grid,
    /// Zone polygon.
    Zone,
    /// Zone label pill (on the floor).
    ZoneLabel,
    /// Edge stroke.
    Edge,
    /// Arrow head (projected triangle).
    ArrowHead,
    /// Animated packet.
    Packet,
    /// Node body (icon).
    Node,
    /// Leader line from node to its label.
    Leader,
    /// Node label box.
    NodeLabel,
    /// Edge label pill.
    EdgeLabel,
    /// Badge (`×N`, `v2`).
    Badge,
    /// Title / subtitle.
    Title,
    /// Story caption.
    Caption,
}

impl ItemRole {
    /// Stable `data-role` value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Grid => "grid",
            Self::Zone => "zone",
            Self::ZoneLabel => "zone-label",
            Self::Edge => "edge",
            Self::ArrowHead => "arrow-head",
            Self::Packet => "packet",
            Self::Node => "node",
            Self::Leader => "leader",
            Self::NodeLabel => "node-label",
            Self::EdgeLabel => "edge-label",
            Self::Badge => "badge",
            Self::Title => "title",
            Self::Caption => "caption",
        }
    }
}

/// `data-id` + `data-role` of a group.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ItemTag {
    /// Spec id (node, zone, edge id).
    pub id: String,
    /// Role.
    pub role: ItemRole,
}

impl ItemTag {
    /// New tag.
    pub fn new(id: impl Into<String>, role: ItemRole) -> Self {
        Self { id: id.into(), role }
    }
}

/// Fill paint.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Paint {
    /// `fill="none"`.
    #[default]
    None,
    /// Solid color.
    Solid(Color),
    /// `url(#id)` of a def (gradient).
    Reference(String),
}

/// Line cap.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LineCap {
    /// Butt (SVG default).
    #[default]
    Butt,
    /// Round.
    Round,
}

/// Stroke parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Stroke {
    /// Color.
    pub color: Color,
    /// Width in px.
    pub width: f64,
    /// Dash pattern `(on, off)` in px.
    pub dash: Option<(f64, f64)>,
    /// Dash offset in px (animation frames).
    pub dash_offset: Option<f64>,
    /// Line cap.
    pub cap: LineCap,
    /// Round joins (all solids use round joins).
    pub round_join: bool,
}

impl Stroke {
    /// Solid stroke with round joins.
    pub fn new(color: Color, width: f64) -> Self {
        Self { color, width, dash: None, dash_offset: None, cap: LineCap::Butt, round_join: true }
    }

    /// With a dash pattern.
    pub fn dashed(mut self, on: f64, off: f64) -> Self {
        self.dash = Some((on, off));
        self
    }

    /// With round caps.
    pub fn round_cap(mut self) -> Self {
        self.cap = LineCap::Round;
        self
    }
}

/// Presentation attributes of a shape.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Style {
    /// Fill.
    pub fill: Paint,
    /// `fill-opacity`.
    pub fill_opacity: Option<f64>,
    /// Stroke.
    pub stroke: Option<Stroke>,
    /// `opacity`.
    pub opacity: Option<f64>,
    /// `filter="url(#id)"`.
    pub filter: Option<String>,
    /// `class` (animated SVG hooks such as `flow`).
    pub class: Option<String>,
}

impl Style {
    /// Solid fill, no stroke.
    pub fn fill(color: Color) -> Self {
        Self { fill: Paint::Solid(color), ..Self::default() }
    }

    /// Stroke only.
    pub fn stroke(stroke: Stroke) -> Self {
        Self { stroke: Some(stroke), ..Self::default() }
    }

    /// Adds a stroke.
    pub fn with_stroke(mut self, stroke: Stroke) -> Self {
        self.stroke = Some(stroke);
        self
    }

    /// Sets `opacity`.
    pub fn with_opacity(mut self, opacity: f64) -> Self {
        self.opacity = Some(opacity);
        self
    }

    /// Sets `fill-opacity`.
    pub fn with_fill_opacity(mut self, opacity: f64) -> Self {
        self.fill_opacity = Some(opacity);
        self
    }

    /// Sets a filter def id.
    pub fn with_filter(mut self, id: impl Into<String>) -> Self {
        self.filter = Some(id.into());
        self
    }

    /// Sets a class.
    pub fn with_class(mut self, class: impl Into<String>) -> Self {
        self.class = Some(class.into());
        self
    }

    fn attributes(&self) -> String {
        let mut out = String::new();
        match &self.fill {
            Paint::None => out.push_str(" fill=\"none\""),
            Paint::Solid(color) => {
                let _ = write!(out, " fill=\"{color}\"");
            }
            Paint::Reference(id) => {
                let _ = write!(out, " fill=\"url(#{id})\"");
            }
        }
        if let Some(opacity) = self.fill_opacity {
            let _ = write!(out, " fill-opacity=\"{}\"", fmt_ratio(opacity));
        }
        if let Some(stroke) = &self.stroke {
            let _ = write!(out, " stroke=\"{}\" stroke-width=\"{}\"", stroke.color, fmt_ratio(stroke.width));
            if let Some((on, off)) = stroke.dash {
                let _ = write!(out, " stroke-dasharray=\"{} {}\"", fmt_num(on), fmt_num(off));
            }
            if let Some(offset) = stroke.dash_offset {
                let _ = write!(out, " stroke-dashoffset=\"{}\"", fmt_num(offset));
            }
            if stroke.cap == LineCap::Round {
                out.push_str(" stroke-linecap=\"round\"");
            }
            if stroke.round_join {
                out.push_str(" stroke-linejoin=\"round\"");
            }
        }
        if let Some(opacity) = self.opacity {
            let _ = write!(out, " opacity=\"{}\"", fmt_ratio(opacity));
        }
        if let Some(filter) = &self.filter {
            let _ = write!(out, " filter=\"url(#{filter})\"");
        }
        if let Some(class) = &self.class {
            let _ = write!(out, " class=\"{}\"", escape_xml(class));
        }
        out
    }
}

/// SVG path data builder. Tracks a conservative bounding box (all points, including control points).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PathData {
    data: String,
    bounds: Rect,
}

impl PathData {
    /// Empty path.
    pub fn new() -> Self {
        Self::default()
    }

    fn separator(&mut self) {
        if !self.data.is_empty() {
            self.data.push(' ');
        }
    }

    /// `M x,y`.
    pub fn move_to(mut self, point: ScreenPoint) -> Self {
        self.separator();
        let _ = write!(self.data, "M{},{}", fmt_num(point.x), fmt_num(point.y));
        self.bounds.include(point);
        self
    }

    /// `L x,y`.
    pub fn line_to(mut self, point: ScreenPoint) -> Self {
        self.separator();
        let _ = write!(self.data, "L{},{}", fmt_num(point.x), fmt_num(point.y));
        self.bounds.include(point);
        self
    }

    /// `C c1 c2 p`.
    pub fn cubic_to(mut self, control1: ScreenPoint, control2: ScreenPoint, point: ScreenPoint) -> Self {
        self.separator();
        let _ = write!(
            self.data,
            "C{},{} {},{} {},{}",
            fmt_num(control1.x),
            fmt_num(control1.y),
            fmt_num(control2.x),
            fmt_num(control2.y),
            fmt_num(point.x),
            fmt_num(point.y)
        );
        for each in [control1, control2, point] {
            self.bounds.include(each);
        }
        self
    }

    /// `A rx,ry 0 large,sweep x,y` (elliptical arc, no x-rotation). The caller tracks the arc extent if
    /// it bulges beyond its endpoints (e.g. cylinder bottoms) via [`PathData::include`].
    pub fn arc_to(mut self, radius_x: f64, radius_y: f64, large_arc: bool, sweep: bool, point: ScreenPoint) -> Self {
        self.separator();
        let _ = write!(
            self.data,
            "A{},{} 0 {} {} {},{}",
            fmt_num(radius_x),
            fmt_num(radius_y),
            u8::from(large_arc),
            u8::from(sweep),
            fmt_num(point.x),
            fmt_num(point.y)
        );
        self.bounds.include(point);
        self
    }

    /// `Z`.
    pub fn close(mut self) -> Self {
        self.separator();
        self.data.push('Z');
        self
    }

    /// Extends the tracked bounds (for arcs and glyph outlines).
    pub fn include(mut self, rect: Rect) -> Self {
        self.bounds = self.bounds.union(rect);
        self
    }

    /// The `d` attribute.
    pub fn as_str(&self) -> &str {
        &self.data
    }

    /// Tracked bounds.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }
}

/// Text anchor.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextAnchor {
    /// Start.
    #[default]
    Start,
    /// Middle.
    Middle,
    /// End.
    End,
}

impl TextAnchor {
    fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Middle => "middle",
            Self::End => "end",
        }
    }
}

/// Text presentation (rendered as `<text>`; the outline mode is produced by `crate::text`).
#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    /// Font size in px.
    pub size: f64,
    /// CSS weight (400, 500, 600, 700).
    pub weight: u16,
    /// Anchor.
    pub anchor: TextAnchor,
    /// Fill color.
    pub fill: Color,
    /// Letter spacing in px.
    pub letter_spacing: f64,
}

/// Attributes of a `<g>`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct GroupAttributes {
    /// `data-id` / `data-role`.
    pub tag: Option<ItemTag>,
    /// `transform`.
    pub transform: Option<Affine>,
    /// `opacity`.
    pub opacity: Option<f64>,
    /// `class`.
    pub class: Option<String>,
}

/// One `<defs>` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SvgDef {
    /// Content-derived id (no global counters).
    pub id: String,
    /// Full markup of the def element.
    pub markup: String,
}

/// Result of drawing on a canvas.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CanvasOutput {
    /// SVG markup (no `<svg>` wrapper, no defs).
    pub body: String,
    /// Defs referenced by the body, sorted by id.
    pub defs: Vec<SvgDef>,
    /// Bounds of everything tracked.
    pub bounds: Rect,
}

/// The canvas. See module docs.
#[derive(Debug, Clone)]
pub struct IsoCanvas {
    projection: Projection,
    body: String,
    defs: BTreeMap<String, String>,
    bounds: Rect,
    open_items: Vec<Rect>,
    open_groups: usize,
    motion_phase: f64,
}

impl IsoCanvas {
    /// New empty canvas.
    pub fn new(projection: Projection) -> Self {
        Self {
            projection,
            body: String::new(),
            defs: BTreeMap::new(),
            bounds: Rect::EMPTY,
            open_items: Vec::new(),
            open_groups: 0,
            motion_phase: 0.0,
        }
    }

    /// Phase offset (share of a cycle) added to every icon motion hook opened on this canvas
    /// (`icons::motion::open_hook`): the scene sets one per node so twin icons move out of step.
    pub fn set_motion_phase(&mut self, phase: f64) {
        self.motion_phase = phase;
    }

    /// See [`IsoCanvas::set_motion_phase`] (0 by default).
    pub fn motion_phase(&self) -> f64 {
        self.motion_phase
    }

    /// The projection in use.
    pub fn projection(&self) -> Projection {
        self.projection
    }

    /// Tile size in px.
    pub fn tile(&self) -> f64 {
        self.projection.tile
    }

    /// Projects without tracking (for measurements and candidates).
    pub fn project_quiet(&self, point: PlanPoint, z: f64) -> ScreenPoint {
        self.projection.project(point, z)
    }

    /// Projects and tracks the result.
    pub fn project(&mut self, point: PlanPoint, z: f64) -> ScreenPoint {
        let screen = self.projection.project(point, z);
        self.track(screen);
        screen
    }

    /// Adds a point to the global bounds and every open item.
    pub fn track(&mut self, point: ScreenPoint) {
        self.bounds.include(point);
        for item in &mut self.open_items {
            item.include(point);
        }
    }

    /// Adds a rectangle to the global bounds and every open item.
    pub fn track_rect(&mut self, rect: Rect) {
        if rect.is_empty() {
            return;
        }
        self.bounds = self.bounds.union(rect);
        for item in &mut self.open_items {
            *item = item.union(rect);
        }
    }

    /// Starts tracking a new item's bounds (nested items also feed their parents).
    pub fn begin_item(&mut self) {
        self.open_items.push(Rect::EMPTY);
    }

    /// Ends the innermost item and returns its bounds (EMPTY when nothing was drawn or no item is open).
    pub fn end_item(&mut self) -> Rect {
        self.open_items.pop().unwrap_or(Rect::EMPTY)
    }

    /// Opens `<g …>`.
    pub fn open_group(&mut self, attributes: &GroupAttributes) {
        self.body.push_str("<g");
        if let Some(tag) = &attributes.tag {
            let _ = write!(self.body, " data-id=\"{}\" data-role=\"{}\"", escape_xml(&tag.id), tag.role.as_str());
        }
        if let Some(transform) = &attributes.transform {
            let _ = write!(self.body, " transform=\"{}\"", fmt_matrix(transform));
        }
        if let Some(opacity) = attributes.opacity {
            let _ = write!(self.body, " opacity=\"{}\"", fmt_ratio(opacity));
        }
        if let Some(class) = &attributes.class {
            let _ = write!(self.body, " class=\"{}\"", escape_xml(class));
        }
        self.body.push('>');
        self.open_groups += 1;
    }

    /// Closes the innermost `<g>` (no-op if none is open).
    pub fn close_group(&mut self) {
        if self.open_groups > 0 {
            self.body.push_str("</g>");
            self.open_groups -= 1;
        }
    }

    /// `<polygon>` with screen points (tracked).
    pub fn polygon(&mut self, points: &[ScreenPoint], style: &Style) {
        for point in points {
            self.track(*point);
        }
        let _ = write!(self.body, "<polygon points=\"{}\"{}/>", fmt_points(points), style.attributes());
    }

    /// `<polygon>` from plan points at height `z` (projected and tracked).
    pub fn plan_polygon(&mut self, points: &[PlanPoint], z: f64, style: &Style) {
        let screen: Vec<ScreenPoint> = points.iter().map(|point| self.projection.project(*point, z)).collect();
        self.polygon(&screen, style);
    }

    /// `<path>`; its tracked bounds are added.
    pub fn path(&mut self, path: &PathData, style: &Style) {
        self.track_rect(path.bounds());
        let _ = write!(self.body, "<path d=\"{}\"{}/>", path.as_str(), style.attributes());
    }

    /// `<line>`.
    pub fn line(&mut self, from: ScreenPoint, to: ScreenPoint, stroke: &Stroke) {
        self.track(from);
        self.track(to);
        let style = Style::stroke(stroke.clone());
        let _ = write!(
            self.body,
            "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"{}/>",
            fmt_num(from.x),
            fmt_num(from.y),
            fmt_num(to.x),
            fmt_num(to.y),
            style.attributes().replacen(" fill=\"none\"", "", 1)
        );
    }

    /// `<circle>`.
    pub fn circle(&mut self, center: ScreenPoint, radius: f64, style: &Style) {
        self.track_rect(Rect::centered(center, 2.0 * radius, 2.0 * radius));
        let _ = write!(
            self.body,
            "<circle cx=\"{}\" cy=\"{}\" r=\"{}\"{}/>",
            fmt_num(center.x),
            fmt_num(center.y),
            fmt_num(radius),
            style.attributes()
        );
    }

    /// `<ellipse>`.
    pub fn ellipse(&mut self, center: ScreenPoint, radius_x: f64, radius_y: f64, style: &Style) {
        self.track_rect(Rect::centered(center, 2.0 * radius_x, 2.0 * radius_y));
        let _ = write!(
            self.body,
            "<ellipse cx=\"{}\" cy=\"{}\" rx=\"{}\" ry=\"{}\"{}/>",
            fmt_num(center.x),
            fmt_num(center.y),
            fmt_num(radius_x),
            fmt_num(radius_y),
            style.attributes()
        );
    }

    /// `<rect>` with rounded corners.
    pub fn rect(&mut self, rect: Rect, corner_radius: f64, style: &Style) {
        self.track_rect(rect);
        let _ = write!(
            self.body,
            "<rect x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            fmt_num(rect.x0),
            fmt_num(rect.y0),
            fmt_num(rect.width()),
            fmt_num(rect.height())
        );
        if corner_radius > 0.0 {
            let _ = write!(self.body, " rx=\"{}\"", fmt_num(corner_radius));
        }
        let _ = write!(self.body, "{}/>", style.attributes());
    }

    /// `<text>` at `position` (baseline). Text extents are NOT known here: the caller tracks the label box
    /// it measured with `crate::text` via [`IsoCanvas::track_rect`].
    pub fn text(&mut self, position: ScreenPoint, content: &str, style: &TextStyle) {
        let _ = write!(
            self.body,
            "<text x=\"{}\" y=\"{}\" font-size=\"{}\" font-weight=\"{}\"",
            fmt_num(position.x),
            fmt_num(position.y),
            fmt_num(style.size),
            style.weight
        );
        if style.anchor != TextAnchor::Start {
            let _ = write!(self.body, " text-anchor=\"{}\"", style.anchor.as_str());
        }
        if style.letter_spacing != 0.0 {
            let _ = write!(self.body, " letter-spacing=\"{}\"", fmt_ratio(style.letter_spacing));
        }
        let _ = write!(self.body, " fill=\"{}\">{}</text>", style.fill, escape_xml(content));
    }

    /// Appends pre-built markup (e.g. text outlines) and tracks `bounds`.
    pub fn raw(&mut self, markup: &str, bounds: Rect) {
        self.track_rect(bounds);
        self.body.push_str(markup);
    }

    /// Registers a def under a content-derived id. The first registration wins; returns the id.
    pub fn define(&mut self, id: impl Into<String>, markup: impl Into<String>) -> String {
        let id = id.into();
        self.defs.entry(id.clone()).or_insert_with(|| markup.into());
        id
    }

    /// Bounds of everything tracked so far.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Closes any open group and returns the output.
    pub fn finish(mut self) -> CanvasOutput {
        while self.open_groups > 0 {
            self.close_group();
        }
        let defs = self.defs.into_iter().map(|(id, markup)| SvgDef { id, markup }).collect();
        CanvasOutput { body: self.body, defs, bounds: self.bounds }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{COS_30, PlanPoint};

    #[test]
    fn numbers_are_fixed_precision() {
        assert_eq!(fmt_num(1.0), "1");
        assert_eq!(fmt_num(1.26), "1.3");
        assert_eq!(fmt_num(-0.04), "0");
        assert_eq!(fmt_num(-12.25), "-12.3");
        assert_eq!(fmt_ratio(0.866_025_4), "0.866");
        assert_eq!(fmt_ratio(0.5), "0.5");
        assert_eq!(fmt_ratio(-0.0), "0");
    }

    #[test]
    fn item_bounds_are_tracked_separately() {
        let mut canvas = IsoCanvas::new(Projection::new(10.0));
        canvas.begin_item();
        canvas.circle(ScreenPoint::new(0.0, 0.0), 5.0, &Style::fill(Color::WHITE));
        let first = canvas.end_item();
        canvas.begin_item();
        canvas.rect(Rect::new(100.0, 100.0, 110.0, 120.0), 2.0, &Style::fill(Color::BLACK));
        let second = canvas.end_item();
        assert_eq!(first, Rect::new(-5.0, -5.0, 5.0, 5.0));
        assert_eq!(second, Rect::new(100.0, 100.0, 110.0, 120.0));
        assert_eq!(canvas.bounds(), Rect::new(-5.0, -5.0, 110.0, 120.0));
    }

    #[test]
    fn nested_items_feed_parents() {
        let mut canvas = IsoCanvas::new(Projection::new(10.0));
        canvas.begin_item();
        canvas.begin_item();
        canvas.track(ScreenPoint::new(3.0, 4.0));
        let inner = canvas.end_item();
        canvas.track(ScreenPoint::new(-1.0, -1.0));
        let outer = canvas.end_item();
        assert_eq!(inner, Rect::new(3.0, 4.0, 3.0, 4.0));
        assert_eq!(outer, Rect::new(-1.0, -1.0, 3.0, 4.0));
        assert!(canvas.end_item().is_empty());
    }

    #[test]
    fn plan_polygon_projects_points() {
        let mut canvas = IsoCanvas::new(Projection::new(56.0));
        canvas.plan_polygon(
            &[PlanPoint::new(0.0, 0.0), PlanPoint::new(1.0, 0.0), PlanPoint::new(1.0, 1.0), PlanPoint::new(0.0, 1.0)],
            0.0,
            &Style::fill(Color::from_u32(0xcdd6f6)).with_fill_opacity(0.75),
        );
        let output = canvas.finish();
        assert_eq!(
            output.body,
            "<polygon points=\"0,0 48.5,28 0,56 -48.5,28\" fill=\"#cdd6f6\" fill-opacity=\"0.75\"/>"
        );
        let half_width = COS_30 * 56.0;
        assert!((output.bounds.x0 + half_width).abs() < 1e-9 && (output.bounds.x1 - half_width).abs() < 1e-9);
        assert!(output.bounds.y0.abs() < 1e-9 && (output.bounds.y1 - 56.0).abs() < 1e-9);
    }

    #[test]
    fn groups_carry_hit_test_tags_and_defs_are_sorted() {
        let mut canvas = IsoCanvas::new(Projection::default());
        canvas.open_group(&GroupAttributes {
            tag: Some(ItemTag::new("api<1>", ItemRole::Node)),
            ..GroupAttributes::default()
        });
        canvas.define("z", "<filter id=\"z\"/>");
        canvas.define("a", "<filter id=\"a\"/>");
        canvas.define("a", "<ignored/>");
        let output = canvas.finish();
        assert_eq!(output.body, "<g data-id=\"api&lt;1&gt;\" data-role=\"node\"></g>");
        let ids: Vec<&str> = output.defs.iter().map(|def| def.id.as_str()).collect();
        assert_eq!(ids, ["a", "z"]);
        assert_eq!(output.defs[0].markup, "<filter id=\"a\"/>");
    }

    #[test]
    fn path_builder_tracks_control_points() {
        let path = PathData::new()
            .move_to(ScreenPoint::new(0.0, 0.0))
            .cubic_to(ScreenPoint::new(10.0, -20.0), ScreenPoint::new(20.0, 20.0), ScreenPoint::new(30.0, 0.0))
            .close();
        assert_eq!(path.as_str(), "M0,0 C10,-20 20,20 30,0 Z");
        assert_eq!(path.bounds(), Rect::new(0.0, -20.0, 30.0, 20.0));
    }
}
