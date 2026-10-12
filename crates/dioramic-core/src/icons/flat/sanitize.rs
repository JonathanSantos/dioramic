//! SVG sanitizer for flat icons: an allow-list copy of the source tree.
//!
//! Provider SVGs come from disk (packs) and may be hostile. Only drawing elements and presentation
//! attributes survive; scripts, event handlers, `<foreignObject>`, `<image>`, `<text>`, external references
//! and editor metadata are dropped. Every `id` (and every CSS class) is prefixed with a content-derived
//! symbol id, so two icons in the same document never collide. `<style>` sheets are not copied: their simple
//! class rules are inlined into each element's `style`, which keeps them from leaking into the diagram.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use roxmltree::{Document, Node, ParsingOptions};

use super::FlatIconError;
use crate::canvas::escape_xml;
use crate::color::Color;

/// Namespace of SVG elements.
const SVG_NAMESPACE: &str = "http://www.w3.org/2000/svg";
/// Namespace of the legacy `xlink:href`.
const XLINK_NAMESPACE: &str = "http://www.w3.org/1999/xlink";
/// Placeholder for the id prefix while the content hash is not known yet.
const PREFIX_PLACEHOLDER: &str = "@@dioramic@@";
/// Largest accepted source (bytes): provider icons are a few KB; this keeps hostile files cheap.
pub const MAX_SVG_BYTES: usize = 512 * 1024;
/// Largest accepted node count (billion-laughs style inputs stop here).
const MAX_XML_NODES: u32 = 20_000;
/// Share of the viewBox a filled rectangle must cover to count as the icon's backdrop.
const BACKDROP_COVERAGE: f64 = 0.95;
/// Channel spread below which a color counts as grey (not an accent).
const GREY_SPREAD: u8 = 24;

/// Elements copied as they are (children are visited recursively).
const ALLOWED_ELEMENTS: &[&str] = &[
    "g",
    "path",
    "rect",
    "circle",
    "ellipse",
    "line",
    "polyline",
    "polygon",
    "defs",
    "linearGradient",
    "radialGradient",
    "stop",
    "clipPath",
    "mask",
    "pattern",
    "symbol",
    "use",
    "filter",
    "feGaussianBlur",
    "feOffset",
    "feBlend",
    "feColorMatrix",
    "feFlood",
    "feComposite",
    "feMerge",
    "feMergeNode",
];

/// Presentation and geometry attributes kept on allowed elements.
const ALLOWED_ATTRIBUTES: &[&str] = &[
    "d",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "fx",
    "fy",
    "fr",
    "width",
    "height",
    "points",
    "transform",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-miterlimit",
    "stroke-dasharray",
    "stroke-dashoffset",
    "stroke-opacity",
    "opacity",
    "clip-path",
    "clip-rule",
    "mask",
    "filter",
    "style",
    "id",
    "class",
    "offset",
    "stop-color",
    "stop-opacity",
    "gradientUnits",
    "gradientTransform",
    "spreadMethod",
    "patternUnits",
    "patternContentUnits",
    "patternTransform",
    "viewBox",
    "preserveAspectRatio",
    "clipPathUnits",
    "maskUnits",
    "maskContentUnits",
    "filterUnits",
    "primitiveUnits",
    "display",
    "visibility",
    "color",
    "href",
    "in",
    "in2",
    "result",
    "stdDeviation",
    "dx",
    "dy",
    "mode",
    "type",
    "values",
    "operator",
    "k1",
    "k2",
    "k3",
    "k4",
    "flood-color",
    "flood-opacity",
    "paint-order",
    "vector-effect",
    "isolation",
    "mix-blend-mode",
];

/// The icon's coordinate box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewBox {
    /// Min x.
    pub x: f64,
    /// Min y.
    pub y: f64,
    /// Width (> 0).
    pub width: f64,
    /// Height (> 0).
    pub height: f64,
}

/// A sanitized icon, ready to be registered as a def and referenced with `<use>`.
#[derive(Debug, Clone, PartialEq)]
pub struct SanitizedSvg {
    /// Content-derived id of the def (`fi` + 12 hex digits). Inner ids start with `<symbol_id>-`.
    pub symbol_id: String,
    /// Coordinate box of the content.
    pub view_box: ViewBox,
    /// `<g id="symbol_id">…</g>`, to be placed in `<defs>`.
    pub markup: String,
    /// Color of a filled rectangle covering the whole view box (AWS-style square icons), if any.
    pub backdrop: Option<Color>,
    /// Most frequent saturated fill (brand color), if any.
    pub accent: Option<Color>,
}

/// Sanitizes an SVG document. Fails on invalid XML, a root that is not `<svg>`, no usable view box,
/// or a source larger than [`MAX_SVG_BYTES`].
pub fn sanitize_svg(source: &str) -> Result<SanitizedSvg, FlatIconError> {
    if source.len() > MAX_SVG_BYTES {
        return Err(FlatIconError::TooLarge { bytes: source.len(), limit: MAX_SVG_BYTES });
    }
    let options = ParsingOptions { allow_dtd: true, nodes_limit: MAX_XML_NODES, ..ParsingOptions::default() };
    let document =
        Document::parse_with_options(source, options).map_err(|error| FlatIconError::Xml(error.to_string()))?;
    let root = document.root_element();
    if root.tag_name().name() != "svg" || !is_svg_namespace(root) {
        return Err(FlatIconError::NotSvg);
    }
    let view_box = view_box_of(root).ok_or(FlatIconError::NoViewBox)?;
    let classes = class_rules(root);
    let mut writer = Writer { classes: &classes, body: String::new(), fills: FillStats::new(view_box) };
    let root_paint = Paint::default().inherit(root, &classes);
    for child in root.children() {
        writer.element(child, &root_paint);
    }
    let digest = fnv1a(writer.body.as_bytes());
    let symbol_id = format!("fi{:012x}", digest & 0xffff_ffff_ffff);
    let body = writer.body.replace(PREFIX_PLACEHOLDER, &format!("{symbol_id}-"));
    let root_style = root_presentation(root);
    let markup = format!("<g id=\"{symbol_id}\"{root_style}>{body}</g>");
    Ok(SanitizedSvg { symbol_id, view_box, markup, backdrop: writer.fills.backdrop, accent: writer.fills.accent() })
}

fn is_svg_namespace(node: Node<'_, '_>) -> bool {
    matches!(node.tag_name().namespace(), None | Some(SVG_NAMESPACE))
}

/// `viewBox`, else `0 0 width height` (units dropped).
fn view_box_of(root: Node<'_, '_>) -> Option<ViewBox> {
    if let Some(numbers) = root.attribute("viewBox").map(parse_numbers).filter(|numbers| numbers.len() == 4) {
        let view_box = ViewBox { x: numbers[0], y: numbers[1], width: numbers[2], height: numbers[3] };
        return (view_box.width > 0.0 && view_box.height > 0.0).then_some(view_box);
    }
    let width = root.attribute("width").and_then(parse_length)?;
    let height = root.attribute("height").and_then(parse_length)?;
    (width > 0.0 && height > 0.0).then_some(ViewBox { x: 0.0, y: 0.0, width, height })
}

fn parse_numbers(text: &str) -> Vec<f64> {
    text.split(|character: char| character.is_whitespace() || character == ',')
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn parse_length(text: &str) -> Option<f64> {
    let number = text.trim().trim_end_matches(|character: char| character.is_ascii_alphabetic() || character == '%');
    number.parse().ok()
}

/// Presentation attributes of the root `<svg>` (fill, stroke…) carried to the wrapping `<g>`.
fn root_presentation(root: Node<'_, '_>) -> String {
    let mut out = String::new();
    for attribute in root.attributes() {
        let name = attribute.name();
        if attribute.namespace().is_none() && is_inherited_presentation(name) && safe_value(attribute.value()) {
            let _ = write!(out, " {name}=\"{}\"", escape_xml(attribute.value()));
        }
    }
    out
}

fn is_inherited_presentation(name: &str) -> bool {
    matches!(
        name,
        "fill" | "fill-rule" | "fill-opacity" | "stroke" | "stroke-width" | "stroke-linecap" | "stroke-linejoin"
    )
}

/// A value without any `url(…)` (used where references are not rewritten).
fn safe_value(value: &str) -> bool {
    !value.to_ascii_lowercase().contains("url(")
}

/// Declarations of simple `.class` rules found in `<style>` elements, by class name.
fn class_rules(root: Node<'_, '_>) -> BTreeMap<String, Vec<(String, String)>> {
    let mut rules: BTreeMap<String, Vec<(String, String)>> = BTreeMap::new();
    for style in root.descendants().filter(|node| node.is_element() && node.tag_name().name() == "style") {
        let css: String = style.children().filter_map(|child| child.text()).collect();
        for (selectors, body) in css_rules(&strip_css_comments(&css)) {
            let declarations = parse_declarations(body);
            for selector in selectors.split(',').map(str::trim) {
                if let Some(class) = simple_class_selector(selector) {
                    rules.entry(class.to_owned()).or_default().extend(declarations.iter().cloned());
                }
            }
        }
    }
    rules
}

fn strip_css_comments(css: &str) -> String {
    let mut out = String::with_capacity(css.len());
    let mut rest = css;
    while let Some(start) = rest.find("/*") {
        out.push_str(&rest[..start]);
        rest = rest[start + 2..].find("*/").map_or("", |end| &rest[start + 2 + end + 2..]);
    }
    out.push_str(rest);
    out
}

/// `selectors { body }` pairs; at-rules and nested blocks are skipped.
fn css_rules(css: &str) -> Vec<(&str, &str)> {
    let mut rules = Vec::new();
    let mut rest = css;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        let selectors = rest[..open].trim();
        let body = &rest[open + 1..open + close];
        if !selectors.starts_with('@') && !body.contains('{') {
            rules.push((selectors, body));
        }
        rest = &rest[open + close + 1..];
    }
    rules
}

/// `.st0` → `st0`; anything else (element, id, descendant selectors) is ignored.
fn simple_class_selector(selector: &str) -> Option<&str> {
    let class = selector.strip_prefix('.')?;
    let valid = !class.is_empty()
        && class.chars().all(|character| character.is_ascii_alphanumeric() || character == '-' || character == '_');
    valid.then_some(class)
}

/// `a: b; c: d` → allowed `(name, value)` pairs, lowercase names.
fn parse_declarations(text: &str) -> Vec<(String, String)> {
    text.split(';')
        .filter_map(|declaration| declaration.split_once(':'))
        .map(|(name, value)| (name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        .filter(|(name, value)| {
            ALLOWED_ATTRIBUTES.contains(&name.as_str())
                && !matches!(name.as_str(), "style" | "id" | "class" | "href")
                && !value.is_empty()
        })
        .collect()
}

/// Paint inherited down the tree, to find the backdrop and the accent.
#[derive(Debug, Clone)]
struct Paint {
    fill: Option<String>,
    opacity: f64,
}

impl Default for Paint {
    fn default() -> Self {
        Self { fill: None, opacity: 1.0 }
    }
}

impl Paint {
    fn inherit(&self, node: Node<'_, '_>, classes: &BTreeMap<String, Vec<(String, String)>>) -> Paint {
        let mut paint = self.clone();
        for (name, value) in effective_style(node, classes) {
            match name.as_str() {
                "fill" => paint.fill = Some(value),
                "opacity" | "fill-opacity" => paint.opacity *= value.parse::<f64>().unwrap_or(1.0),
                _ => {}
            }
        }
        paint
    }

    fn color(&self) -> Option<Color> {
        let fill = self.fill.as_deref().unwrap_or("#000").trim();
        if self.opacity < 0.99 {
            return None;
        }
        Color::parse_hex(fill).or_else(|| named_color(fill))
    }
}

fn named_color(name: &str) -> Option<Color> {
    match name.to_ascii_lowercase().as_str() {
        "white" => Some(Color::WHITE),
        "black" => Some(Color::BLACK),
        _ => None,
    }
}

/// Class rules, then presentation attributes, then the `style` attribute (later wins).
fn effective_style(node: Node<'_, '_>, classes: &BTreeMap<String, Vec<(String, String)>>) -> Vec<(String, String)> {
    let mut declarations = Vec::new();
    for attribute in node.attributes().filter(|attribute| attribute.namespace().is_none()) {
        if is_inherited_presentation(attribute.name()) || attribute.name() == "opacity" {
            declarations.push((attribute.name().to_owned(), attribute.value().to_owned()));
        }
    }
    for class in node.attribute("class").unwrap_or_default().split_whitespace() {
        if let Some(rules) = classes.get(class) {
            declarations.extend(rules.iter().cloned());
        }
    }
    declarations.extend(parse_declarations(node.attribute("style").unwrap_or_default()));
    declarations
}

/// Fill colors seen while copying: the backdrop rectangle and a vote for the accent.
struct FillStats {
    view_box: ViewBox,
    backdrop: Option<Color>,
    votes: BTreeMap<[u8; 3], u32>,
}

impl FillStats {
    fn new(view_box: ViewBox) -> Self {
        Self { view_box, backdrop: None, votes: BTreeMap::new() }
    }

    fn record(&mut self, node: Node<'_, '_>, paint: &Paint, inside_defs: bool) {
        if inside_defs {
            return;
        }
        let Some(color) = paint.color() else { return };
        let name = node.tag_name().name();
        if self.backdrop.is_none() && self.votes.is_empty() && name == "rect" && self.covers_view_box(node) {
            self.backdrop = Some(color);
            return;
        }
        if matches!(name, "path" | "rect" | "circle" | "ellipse" | "polygon") && !is_grey(color) {
            *self.votes.entry([color.r, color.g, color.b]).or_default() += 1;
        }
    }

    fn covers_view_box(&self, node: Node<'_, '_>) -> bool {
        if node.attribute("transform").is_some() || node.attribute("rx").is_some() {
            return false;
        }
        let number = |name: &str| node.attribute(name).and_then(parse_length).unwrap_or(0.0);
        let (x, y, width, height) = (number("x"), number("y"), number("width"), number("height"));
        let overlap_width = (x + width).min(self.view_box.x + self.view_box.width) - x.max(self.view_box.x);
        let overlap_height = (y + height).min(self.view_box.y + self.view_box.height) - y.max(self.view_box.y);
        overlap_width.max(0.0) * overlap_height.max(0.0)
            >= BACKDROP_COVERAGE * self.view_box.width * self.view_box.height
    }

    fn accent(&self) -> Option<Color> {
        // Highest count; ties go to the smallest RGB (BTreeMap order), so the choice is deterministic.
        let mut best: Option<(&[u8; 3], u32)> = None;
        for (rgb, count) in &self.votes {
            if best.is_none_or(|(_, top)| *count > top) {
                best = Some((rgb, *count));
            }
        }
        best.map(|([r, g, b], _)| Color::rgb(*r, *g, *b))
    }
}

fn is_grey(color: Color) -> bool {
    let channels = [color.r, color.g, color.b];
    let spread = channels.iter().max().copied().unwrap_or(0) - channels.iter().min().copied().unwrap_or(0);
    spread < GREY_SPREAD
}

/// Serializes the allowed subset.
struct Writer<'c> {
    classes: &'c BTreeMap<String, Vec<(String, String)>>,
    body: String,
    fills: FillStats,
}

impl Writer<'_> {
    fn element(&mut self, node: Node<'_, '_>, inherited: &Paint) {
        if !node.is_element() || !is_svg_namespace(node) {
            return;
        }
        let name = node.tag_name().name();
        if !ALLOWED_ELEMENTS.contains(&name) {
            return;
        }
        let Some(attributes) = self.attributes(node) else { return };
        let paint = inherited.inherit(node, self.classes);
        let inside_defs = node.ancestors().any(|ancestor| {
            matches!(
                ancestor.tag_name().name(),
                "defs" | "clipPath" | "mask" | "pattern" | "symbol" | "linearGradient" | "radialGradient"
            )
        }) || matches!(name, "defs" | "clipPath" | "mask" | "pattern" | "symbol");
        self.fills.record(node, &paint, inside_defs);
        let _ = write!(self.body, "<{name}{attributes}");
        let children: Vec<Node<'_, '_>> = node.children().filter(Node::is_element).collect();
        if children.is_empty() {
            self.body.push_str("/>");
            return;
        }
        self.body.push('>');
        for child in children {
            self.element(child, &paint);
        }
        let _ = write!(self.body, "</{name}>");
    }

    /// Allowed attributes, with ids/classes prefixed and references rewritten. `None` drops the element
    /// (a `<use>` pointing outside the document).
    fn attributes(&self, node: Node<'_, '_>) -> Option<String> {
        let mut out = String::new();
        let mut style = Vec::new();
        for class in node.attribute("class").unwrap_or_default().split_whitespace() {
            if let Some(rules) = self.classes.get(class) {
                style.extend(rules.iter().cloned());
            }
        }
        for attribute in node.attributes() {
            let name = match attribute.namespace() {
                None => attribute.name(),
                Some(XLINK_NAMESPACE) if attribute.name() == "href" => "href",
                Some(_) => continue,
            };
            if !ALLOWED_ATTRIBUTES.contains(&name) {
                continue;
            }
            let value = attribute.value();
            match name {
                "id" => {
                    let _ = write!(out, " id=\"{PREFIX_PLACEHOLDER}{}\"", escape_xml(value));
                }
                "class" => {
                    let classes: Vec<String> =
                        value.split_whitespace().map(|class| format!("{PREFIX_PLACEHOLDER}{class}")).collect();
                    let _ = write!(out, " class=\"{}\"", escape_xml(&classes.join(" ")));
                }
                "href" => {
                    let target = value.trim().strip_prefix('#')?;
                    let _ = write!(out, " href=\"#{PREFIX_PLACEHOLDER}{}\"", escape_xml(target));
                }
                "style" => style.extend(parse_declarations(value)),
                _ => {
                    if let Some(value) = rewrite_references(value) {
                        let _ = write!(out, " {name}=\"{}\"", escape_xml(&value));
                    }
                }
            }
        }
        let style: Vec<String> = style
            .into_iter()
            .filter_map(|(name, value)| rewrite_references(&value).map(|value| format!("{name}:{value}")))
            .collect();
        if !style.is_empty() {
            let _ = write!(out, " style=\"{}\"", escape_xml(&style.join(";")));
        }
        Some(out)
    }
}

/// Rewrites `url(#id)` to the prefixed id; `None` when the value references anything outside the document
/// (or uses CSS escapes/expressions).
fn rewrite_references(value: &str) -> Option<String> {
    let lower = value.to_ascii_lowercase();
    if lower.contains("expression(") || lower.contains("javascript:") || value.contains('\\') {
        return None;
    }
    let mut out = String::with_capacity(value.len());
    let mut rest = value;
    while let Some(start) = rest.to_ascii_lowercase().find("url(") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 4..];
        let end = after.find(')')?;
        let target = after[..end].trim().trim_matches(|character| character == '\'' || character == '"');
        let id = target.strip_prefix('#')?;
        let _ = write!(out, "url(#{PREFIX_PLACEHOLDER}{id})");
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

/// FNV-1a 64-bit: tiny, deterministic and dependency-free (ids only need to be stable, not secure).
fn fnv1a(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    bytes.iter().fold(OFFSET, |hash, byte| (hash ^ u64::from(*byte)).wrapping_mul(PRIME))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const AWS_LIKE: &str = r##"<?xml version="1.0" encoding="UTF-8"?>
<svg width="80px" height="80px" viewBox="0 0 80 80" xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink">
  <title>Arch_AWS-Lambda_64</title>
  <g id="Icon" stroke="none" fill="none" fill-rule="evenodd">
    <g id="BG" fill="#ED7100"><rect x="0" y="0" width="80" height="80"></rect></g>
    <path d="M28,66 L15,66 Z" fill="#FFFFFF"></path>
  </g>
</svg>"##;

    #[test]
    fn keeps_drawing_and_finds_the_backdrop() {
        let icon = sanitize_svg(AWS_LIKE).unwrap();
        assert_eq!(icon.view_box, ViewBox { x: 0.0, y: 0.0, width: 80.0, height: 80.0 });
        assert_eq!(icon.backdrop, Some(Color::from_u32(0xed7100)));
        assert!(icon.markup.starts_with(&format!("<g id=\"{}\"", icon.symbol_id)));
        assert!(icon.markup.contains("<path d=\"M28,66 L15,66 Z\" fill=\"#FFFFFF\"/>"));
        assert!(icon.markup.contains(&format!("id=\"{}-Icon\"", icon.symbol_id)));
        assert!(!icon.markup.contains("title"));
        assert!(!icon.markup.contains(PREFIX_PLACEHOLDER));
    }

    #[test]
    fn ids_are_stable_and_content_derived() {
        let first = sanitize_svg(AWS_LIKE).unwrap();
        let second = sanitize_svg(AWS_LIKE).unwrap();
        assert_eq!(first.symbol_id, second.symbol_id);
        let other = sanitize_svg(&AWS_LIKE.replace("#ED7100", "#00A1C9")).unwrap();
        assert_ne!(first.symbol_id, other.symbol_id);
    }

    #[test]
    fn strips_scripts_handlers_and_external_references() {
        let hostile = r##"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" viewBox="0 0 10 10" onload="alert(1)">
  <script>alert(1)</script>
  <foreignObject><div xmlns="http://www.w3.org/1999/xhtml">x</div></foreignObject>
  <image href="https://evil.example/x.png" width="10" height="10"/>
  <use xlink:href="https://evil.example/s.svg#a"/>
  <use xlink:href="#local"/>
  <text>hi</text>
  <rect id="local" width="5" height="5" onclick="x()" fill="url(https://evil.example/p)" stroke="url(#grad)"
        style="fill: url(http://x); stroke-width: 2; behavior: url(x.htc)"/>
  <a href="https://evil.example"><path d="M0 0"/></a>
</svg>"##;
        let icon = sanitize_svg(hostile).unwrap();
        let markup = icon.markup.to_ascii_lowercase();
        for banned in
            ["script", "alert", "onload", "onclick", "foreignobject", "image", "evil", "<text", "behavior", "<a"]
        {
            assert!(!markup.contains(banned), "{banned} survived: {markup}");
        }
        assert!(icon.markup.contains(&format!("href=\"#{}-local\"", icon.symbol_id)));
        assert!(icon.markup.contains(&format!("stroke=\"url(#{}-grad)\"", icon.symbol_id)));
        assert!(icon.markup.contains("stroke-width:2"));
    }

    #[test]
    fn inlines_class_rules_and_prefixes_classes() {
        let styled = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512">
  <defs><style>/* c */ .st0 { fill: none; } .st1, .st2 { fill: #4285f4; } path { fill: red } @media print { .st1 { fill: black } }</style></defs>
  <rect class="st0" width="512" height="512"/>
  <path class="st1" d="M0 0"/>
</svg>"#;
        let icon = sanitize_svg(styled).unwrap();
        assert!(!icon.markup.contains("<style"));
        assert!(icon.markup.contains("style=\"fill:#4285f4\""), "{}", icon.markup);
        assert!(icon.markup.contains(&format!("class=\"{}-st1\"", icon.symbol_id)));
        assert!(!icon.markup.contains("red"));
        // The transparent bounding box is not a backdrop; the blue is the accent.
        assert_eq!(icon.backdrop, None);
        assert_eq!(icon.accent, Some(Color::from_u32(0x4285f4)));
    }

    #[test]
    fn falls_back_to_width_and_height_and_rejects_bad_input() {
        let sized = r#"<svg xmlns="http://www.w3.org/2000/svg" width="18mm" height="17.5mm"><path d="M0 0"/></svg>"#;
        let icon = sanitize_svg(sized).unwrap();
        assert_eq!((icon.view_box.width, icon.view_box.height), (18.0, 17.5));
        assert!(matches!(sanitize_svg("<html/>"), Err(FlatIconError::NotSvg)));
        assert!(matches!(sanitize_svg("<svg"), Err(FlatIconError::Xml(_))));
        assert!(matches!(sanitize_svg(r#"<svg xmlns="http://www.w3.org/2000/svg"/>"#), Err(FlatIconError::NoViewBox)));
        let huge = format!("<svg>{}</svg>", " ".repeat(MAX_SVG_BYTES));
        assert!(matches!(sanitize_svg(&huge), Err(FlatIconError::TooLarge { .. })));
    }

    #[test]
    fn inkscape_metadata_is_dropped() {
        let inkscape = r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:sodipodi="http://sodipodi.sourceforge.net/DTD/sodipodi-0.dtd"
  xmlns:inkscape="http://www.inkscape.org/namespaces/inkscape" viewBox="0 0 18 17" inkscape:version="0.91">
  <sodipodi:namedview id="base" pagecolor="white"/>
  <metadata><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"/></metadata>
  <g inkscape:label="Layer 1"><path d="M1 1" style="fill:#326ce5;fill-opacity:1;stroke:none"/></g>
</svg>"#;
        let icon = sanitize_svg(inkscape).unwrap();
        assert!(
            !icon.markup.contains("namedview") && !icon.markup.contains("inkscape") && !icon.markup.contains("rdf")
        );
        assert_eq!(icon.accent, Some(Color::from_u32(0x326ce5)));
    }
}
