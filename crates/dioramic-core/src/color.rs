//! RGB color with the shading operations used by the isometric style (faces 1.0 / 0.8 / 0.62).

use std::fmt;

use dioramic_spec::HexColor;

/// An opaque sRGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Color {
    /// Red.
    pub r: u8,
    /// Green.
    pub g: u8,
    /// Blue.
    pub b: u8,
}

impl Color {
    /// Pure white.
    pub const WHITE: Color = Color::rgb(0xff, 0xff, 0xff);
    /// Pure black.
    pub const BLACK: Color = Color::rgb(0, 0, 0);

    /// Builds a color from components.
    pub const fn rgb(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Builds a color from `0xRRGGBB`.
    pub const fn from_u32(value: u32) -> Self {
        Self::rgb((value >> 16) as u8, (value >> 8) as u8, value as u8)
    }

    /// Parses `#rgb` / `#rrggbb`.
    pub fn parse_hex(text: &str) -> Option<Self> {
        let [r, g, b] = HexColor(text.to_owned()).rgb()?;
        Some(Self::rgb(r, g, b))
    }

    /// Multiplies every channel by `factor` (clamped to 0..=255), like the prototype's `shade`.
    /// `factor < 1` darkens, `> 1` lightens.
    pub fn shade(self, factor: f64) -> Self {
        let scale = |channel: u8| (f64::from(channel) * factor).round().clamp(0.0, 255.0) as u8;
        Self::rgb(scale(self.r), scale(self.g), scale(self.b))
    }

    /// Linear interpolation towards `other` (`amount` 0 → self, 1 → other).
    pub fn mix(self, other: Color, amount: f64) -> Self {
        let amount = amount.clamp(0.0, 1.0);
        let lerp = |a: u8, b: u8| (f64::from(a) + (f64::from(b) - f64::from(a)) * amount).round() as u8;
        Self::rgb(lerp(self.r, other.r), lerp(self.g, other.g), lerp(self.b, other.b))
    }

    /// Lowercase `#rrggbb`.
    pub fn to_hex(self) -> String {
        format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shade_matches_prototype() {
        // shade('#56607a', 0.8) no protótipo = #454d62
        assert_eq!(Color::from_u32(0x56607a).shade(0.8).to_hex(), "#454d62");
        assert_eq!(Color::from_u32(0xf0f0f0).shade(1.2), Color::WHITE);
    }

    #[test]
    fn parses_hex() {
        assert_eq!(Color::parse_hex("#3E50C8"), Some(Color::from_u32(0x3e50c8)));
        assert_eq!(Color::parse_hex("nope"), None);
    }
}
