//! Small enums and value types shared by the raw spec ([`crate::SpecV1`]) and the normalized [`crate::Model`].

use std::borrow::Cow;
use std::fmt;

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Built-in visual theme (see `docs/06-renderizacao.md#temas`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ThemeName {
    /// Light blue-grey background with grid (default).
    #[default]
    Blueprint,
    /// Dark navy background, neon edges with glow.
    Midnight,
    /// Warm off-white, pastel.
    Paper,
    /// Greyscale, no shadows (print).
    Mono,
    /// Light technical drawing: silver icons, thick blue rails with the arrow in the middle, semantic edge colors.
    Schematic,
}

impl ThemeName {
    /// All built-in themes, in documentation order.
    pub const ALL: [ThemeName; 5] = [Self::Blueprint, Self::Midnight, Self::Paper, Self::Mono, Self::Schematic];

    /// Stable lowercase name as written in the spec.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blueprint => "blueprint",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
            Self::Mono => "mono",
            Self::Schematic => "schematic",
        }
    }
}

/// Layout density (`layout.density`). See `docs/06-renderizacao.md#densidade`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Density {
    /// Zones in a single line along the flow, generous gaps (default).
    #[default]
    Spaced,
    /// Zones folded in a serpentine block, smaller gaps (~58% less area).
    Compact,
}

impl Density {
    /// Both densities, `spaced` first.
    pub const ALL: [Density; 2] = [Self::Spaced, Self::Compact];

    /// Stable lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Spaced => "spaced",
            Self::Compact => "compact",
        }
    }
}

/// Main flow axis of the layout (`layout.flow`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FlowAxis {
    /// Sources → sinks along plan `x` (screen: down-right).
    #[default]
    X,
    /// Sources → sinks along plan `y` (screen: down-left).
    Y,
}

/// How zones and nodes are placed on the plan (`layout.algorithm`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LayoutAlgorithm {
    /// Zones in flow order, in a row or folded serpentine (`density`, `wrap`). Default.
    #[default]
    Flow,
    /// Hub-centered and tight: the most connected places in the middle, short edges, aligned on a half-tile
    /// lattice, zones as packed rectangles, sources still towards the start of the flow (`wrap` is ignored).
    Hub,
}

impl LayoutAlgorithm {
    /// Every algorithm, default first.
    pub const ALL: [LayoutAlgorithm; 2] = [Self::Flow, Self::Hub];

    /// Stable lowercase name, as written in the spec.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Flow => "flow",
            Self::Hub => "hub",
        }
    }

    /// True for the default (`flow`).
    pub fn is_default(&self) -> bool {
        *self == Self::Flow
    }
}

/// Zones per row before wrapping (`layout.wrap`): `auto` or a positive integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Wrap {
    /// `auto`: all zones in one row when spaced, ⌈√n⌉ when compact.
    #[default]
    Auto,
    /// Fixed number of zones per row (≥ 1).
    Count(u32),
}

impl Serialize for Wrap {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Auto => serializer.serialize_str("auto"),
            Self::Count(count) => serializer.serialize_u32(*count),
        }
    }
}

impl<'de> Deserialize<'de> for Wrap {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(WrapVisitor)
    }
}

impl JsonSchema for Wrap {
    fn schema_name() -> Cow<'static, str> {
        "Wrap".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Zones per row before folding: \"auto\" or a positive integer.",
            "oneOf": [
                { "type": "integer", "minimum": 1 },
                { "type": "string", "enum": ["auto"] }
            ]
        })
    }
}

struct WrapVisitor;

impl Visitor<'_> for WrapVisitor {
    type Value = Wrap;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a positive integer or \"auto\"")
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        // 0 is accepted here so `validate` can report it as `E_BAD_VALUE` with a path and a hint.
        u32::try_from(value)
            .map(Wrap::Count)
            .map_err(|_| E::custom(format!("invalid wrap {value}: expected a positive integer or auto")))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        let unsigned = u64::try_from(value)
            .map_err(|_| E::custom(format!("invalid wrap {value}: expected a positive integer or auto")))?;
        self.visit_u64(unsigned)
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        if value.eq_ignore_ascii_case("auto") {
            Ok(Wrap::Auto)
        } else {
            Err(E::custom(format!("invalid wrap '{value}': expected a positive integer or auto")))
        }
    }
}

/// Edge routing style (`layout.routing`, `edges[].route`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Routing {
    /// Cubic Bézier leaving the exit port and arriving tangent to the arrow head (default).
    #[default]
    Curved,
    /// Grid-following path with rounded corners.
    Orthogonal,
    /// Straight segment.
    Straight,
    /// Isometric rails (elbows): straight legs along the plan axes only, rounded 90° bends, as few bends as
    /// possible, square into each port and comfortable clearance from the nodes. Also accepted as `elbow`.
    #[serde(alias = "elbow")]
    Rails,
}

/// Where a normalized value came from: written in the spec or filled in by default. Lets the theme supply
/// its own default (`schematic` routes on `rails`) without overriding what the author wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ValueSource {
    /// Not written: the default applies.
    #[default]
    Default,
    /// Written in the spec (or by a caller override).
    Spec,
}

impl ValueSource {
    /// True when the value was not written ([`ValueSource::Default`]).
    pub fn is_default(&self) -> bool {
        *self == Self::Default
    }
}

/// Semantic class of a connection (`edges[].kind`, inferred when absent). See
/// `docs/06-renderizacao.md#cores-semânticas-de-conexão`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    /// Reads and writes to a database, storage or NAS.
    Data,
    /// Cache lookups.
    Cache,
    /// Queues, streams, event buses and async (`..>`) links.
    Messaging,
    /// The internet, third parties, CDN, DNS.
    External,
    /// End-user clients reaching the edge.
    Client,
    /// Service to service (default).
    Service,
    /// Metrics, logs and traces.
    Observability,
}

impl EdgeKind {
    /// Every class, in legend order (the request's path: client → external → service → data side).
    pub const ALL: [EdgeKind; 7] =
        [Self::Client, Self::External, Self::Service, Self::Messaging, Self::Cache, Self::Data, Self::Observability];

    /// Stable lowercase name, as written in the spec and in `styles`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Data => "data",
            Self::Cache => "cache",
            Self::Messaging => "messaging",
            Self::External => "external",
            Self::Client => "client",
            Self::Service => "service",
            Self::Observability => "observability",
        }
    }

    /// Short English name for the legend.
    pub fn title(self) -> &'static str {
        match self {
            Self::Data => "Data",
            Self::Cache => "Cache",
            Self::Messaging => "Messaging",
            Self::External => "External",
            Self::Client => "Client",
            Self::Service => "Service",
            Self::Observability => "Observability",
        }
    }
}

/// How edges are colored (`layout.edge_colors`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum EdgeColors {
    /// By connection class ([`EdgeKind`]); explicit styles and colors still win. Default of `schematic`.
    Semantic,
    /// By style name (`primary`, `secondary`… or `styles`). Default of the other themes.
    Style,
    /// One color for every edge without an explicit color.
    Mono,
}

/// When the edge legend is drawn (`view.legend`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum LegendMode {
    /// With semantic colors and at least two classes in use (default).
    #[default]
    Auto,
    /// Always (with semantic colors).
    On,
    /// Never.
    Off,
}

/// How the background grid is drawn (`view.grid.style`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum GridStyle {
    /// Lines along the plan axes (default).
    #[default]
    Lines,
    /// A dot at every intersection.
    Dots,
    /// No grid.
    None,
}

/// Which ends of an edge carry an arrow head.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ArrowEnds {
    /// Arrow at the target (default).
    #[default]
    End,
    /// Arrow at the source.
    Start,
    /// Arrows at both ends.
    Both,
    /// Plain line.
    None,
}

/// How nodes are distributed inside a zone (`zones[].arrange`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Arrange {
    /// Engine decides (default).
    #[default]
    Auto,
    /// One row along the flow axis.
    Row,
    /// One column across the flow axis.
    Column,
    /// Near-square grid.
    Grid,
}

/// Zone border style.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum BorderStyle {
    /// Dashed outline (default).
    #[default]
    Dashed,
    /// Solid outline.
    Solid,
    /// No outline (fill only).
    None,
}

/// Icon scale (`nodes[].size`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum NodeSize {
    /// Small (×0.75).
    S,
    /// Medium (×1.0, default).
    #[default]
    M,
    /// Large (×1.3).
    L,
}

impl NodeSize {
    /// Scale factor applied to the icon footprint and height.
    pub fn scale(self) -> f64 {
        match self {
            Self::S => 0.75,
            Self::M => 1.0,
            Self::L => 1.3,
        }
    }
}

/// Palette token resolved by each theme into harmonic tones.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ColorToken {
    /// Blue.
    Blue,
    /// Orange.
    Orange,
    /// Teal.
    Teal,
    /// Green.
    Green,
    /// Violet.
    Violet,
    /// Pink.
    Pink,
    /// Red.
    Red,
    /// Gray.
    Gray,
}

impl ColorToken {
    /// All tokens; also the default zone palette rotation order.
    pub const ALL: [ColorToken; 8] =
        [Self::Blue, Self::Orange, Self::Teal, Self::Violet, Self::Green, Self::Pink, Self::Red, Self::Gray];

    /// Stable lowercase name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Blue => "blue",
            Self::Orange => "orange",
            Self::Teal => "teal",
            Self::Green => "green",
            Self::Violet => "violet",
            Self::Pink => "pink",
            Self::Red => "red",
            Self::Gray => "gray",
        }
    }
}

/// A `#rrggbb` (or `#rgb`) literal as written in the spec. Validated by [`crate::validate`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct HexColor(pub String);

impl HexColor {
    /// Parses `#rgb` / `#rrggbb` into `[r, g, b]`. `None` when malformed.
    pub fn rgb(&self) -> Option<[u8; 3]> {
        let digits = self.0.strip_prefix('#')?;
        let expanded: String = match digits.len() {
            3 => digits.chars().flat_map(|c| [c, c]).collect(),
            6 => digits.to_owned(),
            _ => return None,
        };
        let value = u32::from_str_radix(&expanded, 16).ok()?;
        Some([(value >> 16) as u8, (value >> 8) as u8, value as u8])
    }
}

/// A color in the spec: a palette token (preferred) or a hex literal.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum ColorRef {
    /// Theme palette token (`blue`, `orange`…).
    Token(ColorToken),
    /// Hex literal (`#3e50c8`).
    Hex(HexColor),
}

/// Camera rotation in 90° steps. Serialized as the integer `0 | 90 | 180 | 270`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub enum Rotation {
    /// 0° (spec axes as documented).
    #[default]
    R0,
    /// 90° clockwise.
    R90,
    /// 180°.
    R180,
    /// 270° clockwise (= 90° counter-clockwise).
    R270,
}

impl Rotation {
    /// All four rotations in ascending order (the order `auto` evaluates them).
    pub const ALL: [Rotation; 4] = [Self::R0, Self::R90, Self::R180, Self::R270];

    /// Rotation in degrees.
    pub fn degrees(self) -> u16 {
        u16::from(self.quarter_turns()) * 90
    }

    /// Number of 90° steps (0..=3).
    pub fn quarter_turns(self) -> u8 {
        match self {
            Self::R0 => 0,
            Self::R90 => 1,
            Self::R180 => 2,
            Self::R270 => 3,
        }
    }

    /// Builds a rotation from a number of quarter turns (any integer, wrapped mod 4).
    pub fn from_quarter_turns(turns: i32) -> Self {
        match turns.rem_euclid(4) {
            0 => Self::R0,
            1 => Self::R90,
            2 => Self::R180,
            _ => Self::R270,
        }
    }

    /// Parses `0 | 90 | 180 | 270`.
    pub fn from_degrees(degrees: i64) -> Option<Self> {
        match degrees {
            0 => Some(Self::R0),
            90 => Some(Self::R90),
            180 => Some(Self::R180),
            270 => Some(Self::R270),
            _ => None,
        }
    }

    /// Next rotation clockwise (toolbar ⟳ / `]`).
    pub fn clockwise(self) -> Self {
        Self::from_quarter_turns(i32::from(self.quarter_turns()) + 1)
    }

    /// Next rotation counter-clockwise (toolbar ⟲ / `[`).
    pub fn counter_clockwise(self) -> Self {
        Self::from_quarter_turns(i32::from(self.quarter_turns()) - 1)
    }

    /// True for 90° and 270°, where non-square footprints swap width and depth.
    pub fn swaps_axes(self) -> bool {
        self.quarter_turns() % 2 == 1
    }
}

impl fmt::Display for Rotation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.degrees())
    }
}

impl Serialize for Rotation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u16(self.degrees())
    }
}

impl<'de> Deserialize<'de> for Rotation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match deserializer.deserialize_any(RotationVisitor)? {
            RotationSpec::Fixed(rotation) => Ok(rotation),
            RotationSpec::Auto => Err(de::Error::custom("expected 0, 90, 180 or 270")),
        }
    }
}

impl JsonSchema for Rotation {
    fn schema_name() -> Cow<'static, str> {
        "Rotation".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({ "type": "integer", "enum": [0, 90, 180, 270] })
    }
}

/// `view.rotation`: a fixed angle or `auto` (pick the angle with the lowest collision score).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RotationSpec {
    /// Fixed camera angle.
    Fixed(Rotation),
    /// Try all four angles and keep the one with the least residual collision (tie → smaller area).
    Auto,
}

impl Default for RotationSpec {
    fn default() -> Self {
        Self::Fixed(Rotation::R0)
    }
}

impl RotationSpec {
    /// `view.rotation` when the spec omits it: `auto` for [`Density::Compact`] (the folded layout is the one
    /// that changes most between angles, and `auto` costs a few ms), fixed 0° otherwise.
    /// An explicit `rotation` always wins.
    pub fn default_for(density: Density) -> Self {
        match density {
            Density::Compact => Self::Auto,
            Density::Spaced => Self::default(),
        }
    }
}

impl fmt::Display for RotationSpec {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fixed(rotation) => rotation.fmt(f),
            Self::Auto => f.write_str("auto"),
        }
    }
}

impl std::str::FromStr for RotationSpec {
    type Err = String;

    fn from_str(text: &str) -> Result<Self, Self::Err> {
        let trimmed = text.trim();
        if trimmed.eq_ignore_ascii_case("auto") {
            return Ok(Self::Auto);
        }
        trimmed
            .parse::<i64>()
            .ok()
            .and_then(Rotation::from_degrees)
            .map(Self::Fixed)
            .ok_or_else(|| format!("invalid rotation '{trimmed}': expected 0, 90, 180, 270 or auto"))
    }
}

impl Serialize for RotationSpec {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Fixed(rotation) => rotation.serialize(serializer),
            Self::Auto => serializer.serialize_str("auto"),
        }
    }
}

impl<'de> Deserialize<'de> for RotationSpec {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(RotationVisitor)
    }
}

impl JsonSchema for RotationSpec {
    fn schema_name() -> Cow<'static, str> {
        "RotationSpec".into()
    }

    fn json_schema(_generator: &mut SchemaGenerator) -> Schema {
        json_schema!({
            "description": "Camera rotation in 90° steps, or \"auto\" to pick the angle with the fewest collisions.",
            "oneOf": [
                { "type": "integer", "enum": [0, 90, 180, 270] },
                { "type": "string", "enum": ["auto"] }
            ]
        })
    }
}

struct RotationVisitor;

impl Visitor<'_> for RotationVisitor {
    type Value = RotationSpec;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("0, 90, 180, 270 or \"auto\"")
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Self::Value, E> {
        Rotation::from_degrees(value)
            .map(RotationSpec::Fixed)
            .ok_or_else(|| E::custom(format!("invalid rotation {value}: expected 0, 90, 180, 270 or auto")))
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Self::Value, E> {
        let signed = i64::try_from(value).map_err(|_| E::custom("rotation out of range"))?;
        self.visit_i64(signed)
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Self::Value, E> {
        if value.fract() == 0.0 {
            // Truncation is exact here: the value has no fractional part.
            self.visit_i64(value as i64)
        } else {
            Err(E::custom(format!("invalid rotation {value}: expected 0, 90, 180, 270 or auto")))
        }
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
        value.parse().map_err(E::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotation_spec_accepts_integers_and_auto() {
        let fixed: RotationSpec = serde_json::from_str("90").unwrap_or_default();
        assert_eq!(fixed, RotationSpec::Fixed(Rotation::R90));
        let auto: RotationSpec = serde_json::from_str("\"auto\"").unwrap_or_default();
        assert_eq!(auto, RotationSpec::Auto);
        assert!(serde_json::from_str::<RotationSpec>("45").is_err());
    }

    #[test]
    fn rotation_steps_wrap() {
        assert_eq!(Rotation::R270.clockwise(), Rotation::R0);
        assert_eq!(Rotation::R0.counter_clockwise(), Rotation::R270);
        assert!(Rotation::R90.swaps_axes());
    }

    #[test]
    fn hex_color_parses_short_and_long() {
        assert_eq!(HexColor("#3e50c8".into()).rgb(), Some([0x3e, 0x50, 0xc8]));
        assert_eq!(HexColor("#fff".into()).rgb(), Some([255, 255, 255]));
        assert_eq!(HexColor("blue".into()).rgb(), None);
    }
}
