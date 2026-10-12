//! Embedded fonts, text measurement and text-to-outline conversion (ADR-008, `docs/06-renderizacao.md#texto-no-svg-exportado`).
//! Contract: C-005. Implementation of `measure` / `wrap_label` / `outline_path`: T-104.
//!
//! The layout measures with the same font files the rasterizer uses (`dioramic-render` loads
//! [`embedded_fonts`] into its `fontdb`), so what is measured is what is drawn.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use crate::canvas::{PathData, TextAnchor};
use crate::geometry::ScreenPoint;

/// Family used for every diagram label.
pub const LABEL_FAMILY: &str = "Inter";
/// Family of the wordmark / marketing titles.
pub const BRAND_FAMILY: &str = "Space Grotesk";

/// Embedded font family.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FontFamily {
    /// Inter (labels, UI). SIL OFL.
    Inter,
    /// Space Grotesk (brand titles). SIL OFL.
    SpaceGrotesk,
}

impl FontFamily {
    /// CSS family name as stored in the font's name table.
    pub fn css_name(self) -> &'static str {
        match self {
            Self::Inter => LABEL_FAMILY,
            Self::SpaceGrotesk => BRAND_FAMILY,
        }
    }
}

/// One embedded font file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedFont {
    /// Family.
    pub family: FontFamily,
    /// CSS weight.
    pub weight: u16,
    /// TTF bytes.
    pub data: &'static [u8],
}

static EMBEDDED_FONTS: &[EmbeddedFont] = &[
    EmbeddedFont {
        family: FontFamily::Inter,
        weight: 400,
        data: include_bytes!("../../../assets/fonts/Inter-400.ttf"),
    },
    EmbeddedFont {
        family: FontFamily::Inter,
        weight: 500,
        data: include_bytes!("../../../assets/fonts/Inter-500.ttf"),
    },
    EmbeddedFont {
        family: FontFamily::Inter,
        weight: 600,
        data: include_bytes!("../../../assets/fonts/Inter-600.ttf"),
    },
    EmbeddedFont {
        family: FontFamily::Inter,
        weight: 700,
        data: include_bytes!("../../../assets/fonts/Inter-700.ttf"),
    },
    EmbeddedFont {
        family: FontFamily::SpaceGrotesk,
        weight: 500,
        data: include_bytes!("../../../assets/fonts/SpaceGrotesk-500.ttf"),
    },
    EmbeddedFont {
        family: FontFamily::SpaceGrotesk,
        weight: 600,
        data: include_bytes!("../../../assets/fonts/SpaceGrotesk-600.ttf"),
    },
    EmbeddedFont {
        family: FontFamily::SpaceGrotesk,
        weight: 700,
        data: include_bytes!("../../../assets/fonts/SpaceGrotesk-700.ttf"),
    },
];

/// All embedded fonts (never system fonts: determinism across machines).
pub fn embedded_fonts() -> &'static [EmbeddedFont] {
    EMBEDDED_FONTS
}

/// The embedded file closest to `weight` in `family`.
pub fn font_data(family: FontFamily, weight: u16) -> Option<&'static [u8]> {
    EMBEDDED_FONTS
        .iter()
        .filter(|font| font.family == family)
        .min_by_key(|font| font.weight.abs_diff(weight))
        .map(|font| font.data)
}

/// Measured extents of a single line, in px.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    /// Advance width (with kerning and letter spacing).
    pub width: f64,
    /// Ascent above the baseline.
    pub ascent: f64,
    /// Descent below the baseline (positive).
    pub descent: f64,
}

/// Font request for measuring/outlining.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FontSpec {
    /// Family.
    pub family: FontFamily,
    /// CSS weight.
    pub weight: u16,
    /// Size in px.
    pub size: f64,
    /// Extra spacing per character (px).
    pub letter_spacing: f64,
}

/// Fallback advance per character (em fraction) when a font cannot be parsed (prototype `textW`).
const FALLBACK_ADVANCE_EM: f64 = 0.55;
/// Fallback ascent / descent (em fraction).
const FALLBACK_ASCENT_EM: f64 = 0.97;
const FALLBACK_DESCENT_EM: f64 = 0.24;
/// Ellipsis appended to a truncated second line.
const ELLIPSIS: char = '\u{2026}';

/// One shaped glyph, in px relative to the start of the line.
#[derive(Debug, Clone, Copy)]
struct ShapedGlyph {
    id: u16,
    x: f64,
    y: f64,
}

/// A shaped line: glyph positions and total advance (letter spacing applied like resvg: after every
/// cluster except the last).
struct ShapedLine {
    glyphs: Vec<ShapedGlyph>,
    width: f64,
    scale: f64,
}

/// Parsed faces of [`EMBEDDED_FONTS`], built once (parsing the shaping tables dominates measuring).
fn parsed_faces() -> &'static [Option<rustybuzz::Face<'static>>] {
    static FACES: OnceLock<Vec<Option<rustybuzz::Face<'static>>>> = OnceLock::new();
    FACES.get_or_init(|| EMBEDDED_FONTS.iter().map(|font| rustybuzz::Face::from_slice(font.data, 0)).collect())
}

fn parse_face(font: &FontSpec) -> Option<&'static rustybuzz::Face<'static>> {
    let index = EMBEDDED_FONTS
        .iter()
        .enumerate()
        .filter(|(_, embedded)| embedded.family == font.family)
        .min_by_key(|(_, embedded)| embedded.weight.abs_diff(font.weight))
        .map(|(index, _)| index)?;
    parsed_faces().get(index)?.as_ref()
}

fn shape_line(face: &rustybuzz::Face<'_>, text: &str, font: &FontSpec) -> ShapedLine {
    let scale = font.size / f64::from(face.units_per_em().max(1));
    let mut buffer = rustybuzz::UnicodeBuffer::new();
    buffer.push_str(text);
    let shaped = rustybuzz::shape(face, &[], buffer);
    let infos = shaped.glyph_infos();
    let positions = shaped.glyph_positions();
    let cluster_count = count_clusters(infos);
    let mut glyphs = Vec::with_capacity(infos.len());
    let mut pen = 0.0;
    let mut clusters_seen = 0usize;
    for (index, (info, position)) in infos.iter().zip(positions).enumerate() {
        glyphs.push(ShapedGlyph {
            id: u16::try_from(info.glyph_id).unwrap_or(0),
            x: pen + f64::from(position.x_offset) * scale,
            y: f64::from(position.y_offset) * scale,
        });
        pen += f64::from(position.x_advance) * scale;
        let cluster_ends = infos.get(index + 1).is_none_or(|next| next.cluster != info.cluster);
        if cluster_ends {
            clusters_seen += 1;
            if clusters_seen < cluster_count {
                pen += font.letter_spacing;
            }
        }
    }
    ShapedLine { glyphs, width: pen, scale }
}

fn count_clusters(infos: &[rustybuzz::GlyphInfo]) -> usize {
    infos.iter().enumerate().filter(|(index, info)| *index == 0 || infos[index - 1].cluster != info.cluster).count()
}

fn fallback_metrics(text: &str, font: &FontSpec) -> TextMetrics {
    let characters = text.chars().count() as f64;
    TextMetrics {
        width: characters * font.size * FALLBACK_ADVANCE_EM + (characters - 1.0).max(0.0) * font.letter_spacing,
        ascent: font.size * FALLBACK_ASCENT_EM,
        descent: font.size * FALLBACK_DESCENT_EM,
    }
}

/// Measures one line with shaping (`rustybuzz`) on the embedded font.
pub fn measure(text: &str, font: &FontSpec) -> TextMetrics {
    let key = MeasureKey {
        text: text.to_owned(),
        family: font.family,
        weight: font.weight,
        size_bits: font.size.to_bits(),
        spacing_bits: font.letter_spacing.to_bits(),
    };
    let cache = MEASURE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    // A poisoned lock only means another thread panicked mid-insert of a pure value: the map is still valid.
    if let Some(metrics) = cache.lock().unwrap_or_else(PoisonError::into_inner).get(&key).copied() {
        return metrics;
    }
    let metrics = measure_uncached(text, font);
    let mut cache = cache.lock().unwrap_or_else(PoisonError::into_inner);
    if cache.len() >= MEASURE_CACHE_LIMIT {
        cache.clear();
    }
    cache.insert(key, metrics);
    metrics
}

/// Cache key of [`measure`] (exact float bits: a cache hit returns exactly what shaping returned).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct MeasureKey {
    text: String,
    family: FontFamily,
    weight: u16,
    size_bits: u64,
    spacing_bits: u64,
}

/// Entries kept before the cache is dropped (labels repeat across the 4 angles of `rotation: auto`).
const MEASURE_CACHE_LIMIT: usize = 4096;

/// Lookup-only memo shared by all threads (the 4 views of `rotation: auto` build on their own threads and
/// measure the same labels): iteration order never reaches the output, so a HashMap is fine here.
static MEASURE_CACHE: OnceLock<Mutex<HashMap<MeasureKey, TextMetrics>>> = OnceLock::new();

fn measure_uncached(text: &str, font: &FontSpec) -> TextMetrics {
    let Some(face) = parse_face(font) else {
        return fallback_metrics(text, font);
    };
    let line = shape_line(face, text, font);
    TextMetrics {
        width: line.width,
        ascent: f64::from(face.ascender()) * line.scale,
        descent: -f64::from(face.descender()) * line.scale,
    }
}

/// Breaks a label into at most 2 lines near `max_chars` (word boundaries; the 2nd line is ellipsized
/// if still too long). Labels ≤ `max_chars` stay on one line, and so does a single word with no space
/// to break at (cutting inside a word reads worse than a wide box).
pub fn wrap_label(text: &str, max_chars: usize) -> Vec<String> {
    let text = text.trim();
    let max_chars = max_chars.max(1);
    if text.chars().count() <= max_chars {
        return vec![text.to_owned()];
    }
    let words: Vec<&str> = text.split_whitespace().collect();
    if words.len() < 2 {
        return vec![text.to_owned()];
    }
    let split = best_split(&words, max_chars);
    let first = words[..split].join(" ");
    let second = ellipsize(&words[split..].join(" "), max_chars);
    vec![first, second]
}

/// Index of the first word of line 2: the split whose longest line is shortest, among splits whose first
/// line fits (or the 1-word split when even the first word is too long).
fn best_split(words: &[&str], max_chars: usize) -> usize {
    let line_length = |slice: &[&str]| slice.iter().map(|word| word.chars().count()).sum::<usize>() + slice.len() - 1;
    (1..words.len())
        .filter(|&split| split == 1 || line_length(&words[..split]) <= max_chars)
        .min_by_key(|&split| (line_length(&words[..split]).max(line_length(&words[split..])), std::cmp::Reverse(split)))
        .unwrap_or(1)
}

fn ellipsize(line: &str, max_chars: usize) -> String {
    if line.chars().count() <= max_chars {
        return line.to_owned();
    }
    let kept: String = line.chars().take(max_chars.saturating_sub(1)).collect();
    format!("{}{ELLIPSIS}", kept.trim_end())
}

/// Converts a line of text into an SVG path (glyph outlines via `ttf-parser`), positioned like
/// `<text x y text-anchor>` with baseline at `origin.y`. Used by the "text as outlines" SVG mode.
pub fn outline_path(text: &str, font: &FontSpec, origin: ScreenPoint, anchor: TextAnchor) -> PathData {
    let Some(face) = parse_face(font) else {
        return PathData::new();
    };
    let line = shape_line(face, text, font);
    let start_x = origin.x - anchor_shift(anchor, line.width);
    let mut builder = OutlineSink { path: PathData::new(), current: origin, start: origin, origin, scale: line.scale };
    for glyph in &line.glyphs {
        builder.origin = ScreenPoint::new(start_x + glyph.x, origin.y - glyph.y);
        let _ = face.outline_glyph(rustybuzz::ttf_parser::GlyphId(glyph.id), &mut builder);
    }
    builder.path
}

fn anchor_shift(anchor: TextAnchor, width: f64) -> f64 {
    match anchor {
        TextAnchor::Start => 0.0,
        TextAnchor::Middle => width / 2.0,
        TextAnchor::End => width,
    }
}

/// Receives glyph outlines in font units (y up) and writes them in screen px (y down).
struct OutlineSink {
    path: PathData,
    current: ScreenPoint,
    start: ScreenPoint,
    origin: ScreenPoint,
    scale: f64,
}

impl OutlineSink {
    fn to_screen(&self, x: f32, y: f32) -> ScreenPoint {
        ScreenPoint::new(self.origin.x + f64::from(x) * self.scale, self.origin.y - f64::from(y) * self.scale)
    }

    fn edit(&mut self, change: impl FnOnce(PathData) -> PathData) {
        self.path = change(std::mem::take(&mut self.path));
    }
}

impl rustybuzz::ttf_parser::OutlineBuilder for OutlineSink {
    fn move_to(&mut self, x: f32, y: f32) {
        let point = self.to_screen(x, y);
        self.edit(|path| path.move_to(point));
        self.current = point;
        self.start = point;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        let point = self.to_screen(x, y);
        self.edit(|path| path.line_to(point));
        self.current = point;
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // SVG paths here only speak cubic: a quadratic is the cubic with controls at 2/3 of the way.
        let control = self.to_screen(x1, y1);
        let point = self.to_screen(x, y);
        let current = self.current;
        let control1 = current + (control - current) * (2.0 / 3.0);
        let control2 = point + (control - point) * (2.0 / 3.0);
        self.edit(|path| path.cubic_to(control1, control2, point));
        self.current = point;
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        let control1 = self.to_screen(x1, y1);
        let control2 = self.to_screen(x2, y2);
        let point = self.to_screen(x, y);
        self.edit(|path| path.cubic_to(control1, control2, point));
        self.current = point;
    }

    fn close(&mut self) {
        self.edit(PathData::close);
        self.current = self.start;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fonts_are_embedded_and_parse() {
        assert_eq!(embedded_fonts().len(), 7);
        for font in embedded_fonts() {
            assert!(ttf_parser::Face::parse(font.data, 0).is_ok(), "{:?} {}", font.family, font.weight);
        }
        assert!(font_data(FontFamily::Inter, 550).is_some());
    }

    fn inter(size: f64, weight: u16) -> FontSpec {
        FontSpec { family: FontFamily::Inter, weight, size, letter_spacing: 0.0 }
    }

    #[test]
    fn load_balancer_width_matches_resvg() {
        // Reference: resvg 0.48 bbox advance of "Load Balancer" in Inter 500 at 14 px, measured with the
        // embedded fonts (see the T-104 handoff). The contract tolerance is ±1 px.
        let metrics = measure("Load Balancer", &inter(14.0, 500));
        assert!((metrics.width - RESVG_LOAD_BALANCER_WIDTH).abs() <= 1.0, "{}", metrics.width);
        assert!(metrics.ascent > 10.0 && metrics.descent > 2.0);
    }

    const RESVG_LOAD_BALANCER_WIDTH: f64 = 94.79;

    #[test]
    fn letter_spacing_is_applied_between_clusters_only() {
        let plain = measure("EDGE", &inter(12.0, 700));
        let spaced = measure("EDGE", &FontSpec { letter_spacing: 0.6, ..inter(12.0, 700) });
        assert!((spaced.width - plain.width - 3.0 * 0.6).abs() < 1e-9);
    }

    #[test]
    fn bolder_and_bigger_text_is_wider() {
        let regular = measure("Postgres", &inter(14.0, 400));
        assert!(measure("Postgres", &inter(14.0, 700)).width > regular.width);
        assert!(measure("Postgres", &inter(20.0, 400)).width > regular.width);
        assert_eq!(measure("", &inter(14.0, 400)).width, 0.0);
    }

    #[test]
    fn short_labels_stay_on_one_line() {
        assert_eq!(wrap_label("Load Balancer", 20), vec!["Load Balancer"]);
        assert_eq!(wrap_label("  Redis ", 4), vec!["Redis"]);
    }

    #[test]
    fn long_labels_break_at_the_most_balanced_word_boundary() {
        assert_eq!(wrap_label("Branch Manager Workstation", 20), vec!["Branch Manager", "Workstation"]);
        assert_eq!(wrap_label("MPLS / Internet Uplink", 20), vec!["MPLS / Internet", "Uplink"]);
    }

    #[test]
    fn second_line_is_ellipsized_and_single_words_are_kept() {
        let lines = wrap_label("Primary database cluster with synchronous replicas", 16);
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with(ELLIPSIS));
        assert!(lines.iter().all(|line| line.chars().count() <= 16));
        assert_eq!(wrap_label("Supercalifragilistic", 8), vec!["Supercalifragilistic"]);
    }

    #[test]
    fn outlines_follow_the_anchor() {
        let font = inter(14.0, 500);
        let width = measure("Users", &font).width;
        let start = outline_path("Users", &font, ScreenPoint::new(100.0, 50.0), TextAnchor::Start).bounds();
        let middle = outline_path("Users", &font, ScreenPoint::new(100.0, 50.0), TextAnchor::Middle).bounds();
        assert!(!start.is_empty());
        assert!((start.x0 - middle.x0 - width / 2.0).abs() < 0.11);
        assert!(start.y1 <= 50.5 && start.y0 > 50.0 - 14.0, "{start:?}");
        assert!(outline_path("Users", &font, ScreenPoint::new(0.0, 0.0), TextAnchor::End).bounds().x1 <= 0.5);
    }
}
