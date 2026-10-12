//! Outlines the `<text>` elements inside pre-drawn node markup (T-350).
//!
//! Node bodies are drawn once by the icon drawers through `IsoCanvas`, before the SVG writer knows the
//! text mode; the replica pill (`×3`) is written there as `<text>`. In `TextRendering::Outlines` no
//! `<text>` may reach the document, so the writer rewrites each one into a `<path>` of its glyphs, placed
//! exactly like the `<text x y text-anchor>` it replaces (same rule as the scene labels).

use std::borrow::Cow;
use std::fmt::Write as _;

use crate::canvas::TextAnchor;
use crate::geometry::ScreenPoint;
use crate::text::{self, FontFamily, FontSpec};

const OPEN: &str = "<text";
const CLOSE: &str = "</text>";
/// Values used when the canvas left an attribute out (its own defaults).
const DEFAULT_WEIGHT: u16 = 400;
const DEFAULT_FILL: &str = "#000000";

/// `markup` with every `<text …>…</text>` replaced by a glyph-outline `<path>`. Borrowed when there is
/// no text (the common case: most icons have none).
pub fn outline_inline_text(markup: &str) -> Cow<'_, str> {
    if !markup.contains(OPEN) {
        return Cow::Borrowed(markup);
    }
    let mut output = String::with_capacity(markup.len());
    let mut rest = markup;
    while let Some(element) = next_text_element(rest) {
        output.push_str(&rest[..element.start]);
        output.push_str(&element.outline());
        rest = &rest[element.end..];
    }
    output.push_str(rest);
    Cow::Owned(output)
}

/// One `<text>` element found in markup: its byte range, attributes and content.
struct TextElement<'a> {
    start: usize,
    end: usize,
    attributes: &'a str,
    content: &'a str,
}

fn next_text_element(markup: &str) -> Option<TextElement<'_>> {
    let start = markup.find(OPEN)?;
    let after_name = start + OPEN.len();
    let tag_end = after_name + markup[after_name..].find('>')?;
    let close = tag_end + markup[tag_end..].find(CLOSE)?;
    Some(TextElement {
        start,
        end: close + CLOSE.len(),
        attributes: &markup[after_name..tag_end],
        content: &markup[tag_end + 1..close],
    })
}

impl TextElement<'_> {
    /// The `<path>` drawing this text's glyphs.
    fn outline(&self) -> String {
        let number = |name: &str| attribute(self.attributes, name).and_then(|value| value.parse::<f64>().ok());
        let origin = ScreenPoint::new(number("x").unwrap_or(0.0), number("y").unwrap_or(0.0));
        let font = FontSpec {
            family: FontFamily::Inter,
            weight: attribute(self.attributes, "font-weight")
                .and_then(|value| value.parse().ok())
                .unwrap_or(DEFAULT_WEIGHT),
            size: number("font-size").unwrap_or_default(),
            letter_spacing: number("letter-spacing").unwrap_or(0.0),
        };
        let anchor = match attribute(self.attributes, "text-anchor") {
            Some("middle") => TextAnchor::Middle,
            Some("end") => TextAnchor::End,
            _ => TextAnchor::Start,
        };
        let glyphs = text::outline_path(&unescape_xml(self.content), &font, origin, anchor);
        let fill = attribute(self.attributes, "fill").unwrap_or(DEFAULT_FILL);
        let mut path = String::new();
        let _ = write!(path, "<path d=\"{}\" fill=\"{fill}\"/>", glyphs.as_str());
        path
    }
}

/// Value of `name="…"` in an attribute list (exact name match, so `x` never matches `rx`).
fn attribute<'a>(attributes: &'a str, name: &str) -> Option<&'a str> {
    let mut rest = attributes;
    while let Some(equals) = rest.find("=\"") {
        let key = rest[..equals].rsplit(char::is_whitespace).next().unwrap_or_default();
        let value_start = equals + 2;
        let value_end = value_start + rest[value_start..].find('"')?;
        if key == name {
            return Some(&rest[value_start..value_end]);
        }
        rest = &rest[value_end + 1..];
    }
    None
}

/// Reverses `canvas::escape_xml`.
fn unescape_xml(text: &str) -> Cow<'_, str> {
    if !text.contains('&') {
        return Cow::Borrowed(text);
    }
    Cow::Owned(
        text.replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_without_text_is_borrowed() {
        assert!(matches!(outline_inline_text("<rect x=\"1\"/>"), Cow::Borrowed(_)));
    }

    #[test]
    fn text_becomes_the_same_outline_as_the_labels() {
        let markup = "<g><rect x=\"0\"/><text x=\"40\" y=\"20\" font-size=\"10\" font-weight=\"700\" text-anchor=\"middle\" fill=\"#ffffff\">×3</text></g>";
        let outlined = outline_inline_text(markup);
        assert!(!outlined.contains("<text"), "{outlined}");
        assert!(outlined.starts_with("<g><rect x=\"0\"/><path d=\"M"), "{outlined}");
        assert!(outlined.ends_with("fill=\"#ffffff\"/></g>"), "{outlined}");
        let font = FontSpec { family: FontFamily::Inter, weight: 700, size: 10.0, letter_spacing: 0.0 };
        let expected = text::outline_path("×3", &font, ScreenPoint::new(40.0, 20.0), TextAnchor::Middle);
        assert!(outlined.contains(expected.as_str()));
    }

    #[test]
    fn every_text_element_is_replaced_and_entities_are_decoded() {
        let markup = "<text x=\"0\" y=\"0\" font-size=\"10\" font-weight=\"400\" fill=\"#000000\">a&amp;b</text>|<text x=\"5\" y=\"5\" font-size=\"10\" font-weight=\"400\" fill=\"#000000\">c</text>";
        let outlined = outline_inline_text(markup);
        assert_eq!(outlined.matches("<path").count(), 2);
        assert!(outlined.contains("/>|<path"));
        let font = FontSpec { family: FontFamily::Inter, weight: 400, size: 10.0, letter_spacing: 0.0 };
        let expected = text::outline_path("a&b", &font, ScreenPoint::new(0.0, 0.0), TextAnchor::Start);
        assert!(outlined.contains(expected.as_str()));
    }

    #[test]
    fn attribute_names_match_exactly() {
        let attributes = " rx=\"3\" x=\"7\" font-size=\"9\"";
        assert_eq!(attribute(attributes, "x"), Some("7"));
        assert_eq!(attribute(attributes, "size"), None);
        assert_eq!(attribute(attributes, "font-size"), Some("9"));
    }
}
