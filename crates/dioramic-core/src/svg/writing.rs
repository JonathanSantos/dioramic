//! Label boxes, pills, title and text (as `<text>` or as glyph outlines), and edge path data.

use crate::canvas::{
    GroupAttributes, IsoCanvas, ItemRole, ItemTag, Paint, PathData, Stroke, Style, TextAnchor, TextStyle,
};
use crate::color::Color;
use crate::geometry::{Rect, ScreenPoint};
use crate::scene::framing::{SUBTITLE_STEP_Y, subtitle_font, title_font};
use crate::scene::labels::{
    NODE_LABEL_BASELINE, NODE_LABEL_LINE_STEP, PILL_LINE_STEP, ZONE_LABEL_BASELINE, ZONE_LABEL_PADDING_X,
    node_label_font, pill_font, zone_label_font,
};
use crate::scene::legend::legend_font;
use crate::scene::{EdgePath, PlacedLabel, Scene, SceneLegend};
use crate::text::{self, FontSpec};
use crate::theme::{DashPattern, LinePattern, Theme};

use super::TextRendering;

/// Leader line width (px).
pub const LEADER_WIDTH: f64 = 1.2;
/// Node label box border width and corner radius (px).
const NODE_LABEL_BORDER_WIDTH: f64 = 1.2;
const NODE_LABEL_RADIUS: f64 = 7.0;
/// Pill corner radius and the first baseline below the pill top (px).
const PILL_RADIUS: f64 = 5.0;
const PILL_BASELINE: f64 = 14.0;
/// Hairline border of pills on light themes (px).
const PILL_BORDER_WIDTH: f64 = 1.0;
/// Legend box corner radius (px), dash of a dashed sample (on, off) and dot spacing in stroke widths.
const LEGEND_RADIUS: f64 = 6.0;
const LEGEND_DASH: (f64, f64) = (5.0, 4.0);
const LEGEND_DOT_SPACING: f64 = 2.4;
/// Zone label corner radius (px).
const ZONE_LABEL_RADIUS: f64 = 4.0;
/// Corner radius of orthogonal routes (px).
const ROUTE_CORNER_RADIUS: f64 = 10.0;

/// Writes labels with the theme typography in the requested text mode.
pub struct LabelWriter<'a> {
    /// Theme.
    pub theme: &'a Theme,
    /// Text mode.
    pub text: TextRendering,
}

impl LabelWriter<'_> {
    /// One line of text at `origin` (baseline).
    fn line(
        &self,
        canvas: &mut IsoCanvas,
        content: &str,
        font: &FontSpec,
        origin: ScreenPoint,
        anchor: TextAnchor,
        fill: Color,
    ) {
        match self.text {
            TextRendering::Text => {
                let family = weight_family(font);
                if let Some(family) = &family {
                    canvas.raw(&format!("<g font-family=\"{family}\">"), Rect::EMPTY);
                }
                let style = TextStyle {
                    size: font.size,
                    weight: font.weight,
                    anchor,
                    fill,
                    letter_spacing: font.letter_spacing,
                };
                canvas.text(origin, content, &style);
                if family.is_some() {
                    canvas.raw("</g>", Rect::EMPTY);
                }
            }
            TextRendering::Outlines => {
                let glyphs = text::outline_path(content, font, origin, anchor);
                canvas.path(&glyphs, &Style { fill: Paint::Solid(fill), ..Style::default() });
            }
        }
    }

    /// White rounded box with centered lines.
    pub fn node_label(&self, canvas: &mut IsoCanvas, label: &PlacedLabel) {
        let colors = self.theme.label;
        let style = Style::fill(colors.background).with_stroke(Stroke::new(colors.border, NODE_LABEL_BORDER_WIDTH));
        canvas.rect(label.rect, NODE_LABEL_RADIUS, &style);
        let font = node_label_font(self.theme);
        let center_x = label.rect.center().x;
        for (index, content) in label.lines.iter().enumerate() {
            let baseline = label.rect.y0 + NODE_LABEL_BASELINE + index as f64 * NODE_LABEL_LINE_STEP;
            self.line(canvas, content, &font, ScreenPoint::new(center_x, baseline), TextAnchor::Middle, colors.text);
        }
    }

    /// Dark pill with centered lines.
    pub fn pill(&self, canvas: &mut IsoCanvas, label: &PlacedLabel) {
        let colors = self.theme.pill;
        canvas.rect(label.rect, PILL_RADIUS, &self.pill_style());
        let font = pill_font(self.theme);
        let center_x = label.rect.center().x;
        for (index, content) in label.lines.iter().enumerate() {
            let baseline = label.rect.y0 + PILL_BASELINE + index as f64 * PILL_LINE_STEP;
            self.line(canvas, content, &font, ScreenPoint::new(center_x, baseline), TextAnchor::Middle, colors.text);
        }
    }

    /// Pill lying on the floor (local box under the label's floor transform).
    pub fn zone_label(&self, canvas: &mut IsoCanvas, label: &PlacedLabel) {
        let colors = self.theme.pill;
        canvas.open_group(&GroupAttributes { transform: label.transform, ..GroupAttributes::default() });
        let (width, height) = label.size;
        canvas.rect(Rect::new(0.0, 0.0, width, height), ZONE_LABEL_RADIUS, &self.pill_style());
        let font = zone_label_font(self.theme);
        if let Some(content) = label.lines.first() {
            let origin = ScreenPoint::new(ZONE_LABEL_PADDING_X, ZONE_LABEL_BASELINE);
            self.line(canvas, content, &font, origin, TextAnchor::Start, colors.text);
        }
        canvas.close_group();
    }

    /// Fill (and hairline border, when the theme has one) of a pill.
    fn pill_style(&self) -> Style {
        let colors = self.theme.pill;
        let style = Style::fill(colors.background);
        match colors.border {
            Some(border) => style.with_stroke(Stroke::new(border, PILL_BORDER_WIDTH)),
            None => style,
        }
    }

    /// Edge legend (T-409): a pill-styled box with one stroke sample and name per class.
    pub fn legend(&self, canvas: &mut IsoCanvas, legend: &SceneLegend) {
        canvas.raw("<g data-role=\"legend\">", Rect::EMPTY);
        canvas.rect(legend.rect, LEGEND_RADIUS, &self.pill_style());
        let font = legend_font();
        for entry in &legend.entries {
            let stroke = Stroke::new(entry.color, entry.width).round_cap();
            let stroke = match (entry.line, self.theme.edge_drawing.dash) {
                (LinePattern::Solid, _) => stroke,
                (LinePattern::Dashed, DashPattern::Dashes) => stroke.dashed(LEGEND_DASH.0, LEGEND_DASH.1),
                (LinePattern::Dashed, DashPattern::Dots) => stroke.dashed(0.0, entry.width * LEGEND_DOT_SPACING),
            };
            canvas.line(entry.swatch.0, entry.swatch.1, &stroke);
            self.line(canvas, &entry.name, &font, entry.text_origin, TextAnchor::Start, self.theme.pill.text);
        }
        canvas.raw("</g>", Rect::EMPTY);
    }

    /// Title and subtitle.
    pub fn title(&self, canvas: &mut IsoCanvas, scene: &Scene) {
        let Some(title) = &scene.title else { return };
        canvas.open_group(&GroupAttributes {
            tag: Some(ItemTag::new("title", ItemRole::Title)),
            ..GroupAttributes::default()
        });
        self.line(canvas, &title.title, &title_font(self.theme), title.position, TextAnchor::Start, self.theme.ink);
        if let Some(subtitle) = &title.subtitle {
            let origin = ScreenPoint::new(title.position.x, title.position.y + SUBTITLE_STEP_Y);
            self.line(canvas, subtitle, &subtitle_font(self.theme), origin, TextAnchor::Start, self.theme.subtle);
        }
        canvas.close_group();
    }
}

/// Family list for weights whose static font file names its family after the weight.
///
/// The embedded Inter/Space Grotesk files are static instances: their legacy family name (the one fontdb
/// and resvg match) is "Inter Medium" / "Inter SemiBold", not "Inter", so `font-family="Inter"
/// font-weight="500"` would silently render Regular. Naming the instance first keeps resvg on the
/// measured face, and browsers fall back to "Inter" + `font-weight`, then to a generic sans-serif when
/// Inter is not installed (T-314: without it the group would fall back to the browser's serif).
fn weight_family(font: &FontSpec) -> Option<String> {
    let suffix = match font.weight {
        500 => "Medium",
        600 => "SemiBold",
        _ => return None,
    };
    let family = font.family.css_name();
    Some(format!("{family} {suffix}, {family}, {GENERIC_FAMILY}"))
}

/// Generic family closing every `font-family` list.
const GENERIC_FAMILY: &str = "sans-serif";

/// SVG path data of an edge (orthogonal corners rounded).
pub fn path_data(path: &EdgePath) -> PathData {
    match path {
        EdgePath::Curve(curve) => {
            let [p0, p1, p2, p3] = curve.points;
            PathData::new().move_to(p0).cubic_to(p1, p2, p3)
        }
        EdgePath::Polyline(points) => rounded_polyline(points),
    }
}

fn rounded_polyline(points: &[ScreenPoint]) -> PathData {
    let Some(&first) = points.first() else {
        return PathData::new();
    };
    let mut data = PathData::new().move_to(first);
    for window in points.windows(3) {
        let [before, corner, after] = [window[0], window[1], window[2]];
        let radius = ROUTE_CORNER_RADIUS.min(before.distance(corner) / 2.0).min(after.distance(corner) / 2.0);
        let entry = corner + (before - corner).normalized() * radius;
        let exit = corner + (after - corner).normalized() * radius;
        // A quadratic corner written as its equivalent cubic (controls at 2/3 toward the corner).
        let control1 = entry + (corner - entry) * (2.0 / 3.0);
        let control2 = exit + (corner - exit) * (2.0 / 3.0);
        data = data.line_to(entry).cubic_to(control1, control2, exit);
    }
    if points.len() > 1 {
        data = data.line_to(points[points.len() - 1]);
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn polyline_corners_are_rounded() {
        let points = [ScreenPoint::new(0.0, 0.0), ScreenPoint::new(100.0, 0.0), ScreenPoint::new(100.0, 100.0)];
        let data = rounded_polyline(&points);
        assert_eq!(data.as_str(), "M0,0 L90,0 C96.7,0 100,3.3 100,10 L100,100");
    }
}
