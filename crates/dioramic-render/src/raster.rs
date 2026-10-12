//! SVG → pixmap with resvg and the embedded fonts.

use resvg::tiny_skia::{Pixmap, Transform};
use resvg::usvg;

use crate::MAX_SIDE_PX;
use crate::error::RenderError;
use crate::fonts::usvg_options;

/// How big the raster is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RasterSize {
    /// Multiply the SVG size by this factor (PNG `scale`, 1–4).
    Scale(f64),
    /// Fit this width, keeping the aspect ratio (GIF 1600 px).
    Width(u32),
}

/// Pixel dimensions resolved for one SVG size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PixelSize {
    pub(crate) width: u32,
    pub(crate) height: u32,
}

impl RasterSize {
    /// Resolves the output pixel size of an SVG of `svg_width × svg_height` user units, enforcing
    /// [`MAX_SIDE_PX`].
    pub(crate) fn resolve(self, svg_width: f64, svg_height: f64) -> Result<PixelSize, RenderError> {
        if !(svg_width > 0.0 && svg_height > 0.0) {
            return Err(RenderError::Raster(format!("SVG has an empty size ({svg_width}×{svg_height})")));
        }
        let factor = match self {
            Self::Scale(scale) if scale.is_finite() && scale > 0.0 => scale,
            Self::Scale(scale) => return Err(RenderError::Request(format!("scale must be > 0, got {scale}"))),
            Self::Width(0) => return Err(RenderError::Request("width must be > 0".to_owned())),
            Self::Width(width) => f64::from(width) / svg_width,
        };
        let width = match self {
            Self::Width(width) => f64::from(width),
            Self::Scale(_) => (svg_width * factor).round(),
        };
        let height = (svg_height * factor).round();
        Ok(PixelSize { width: checked_side("width", width)?, height: checked_side("height", height)? })
    }
}

fn checked_side(what: &'static str, pixels: f64) -> Result<u32, RenderError> {
    let limit = u64::from(MAX_SIDE_PX);
    if !pixels.is_finite() || pixels > limit as f64 {
        let actual = if pixels.is_finite() { pixels as u64 } else { u64::MAX };
        return Err(RenderError::Limit { what, actual, limit });
    }
    Ok((pixels as u32).max(1))
}

/// Parses `svg` (with [`crate::fonts::usvg_options`]) and rasterizes it. Errors when a side exceeds
/// [`crate::MAX_SIDE_PX`].
pub fn rasterize(svg: &str, size: RasterSize) -> Result<Pixmap, RenderError> {
    let tree = parse_svg(svg)?;
    rasterize_tree(&tree, size)
}

/// Parses an SVG document with the embedded fonts.
pub(crate) fn parse_svg(svg: &str) -> Result<usvg::Tree, RenderError> {
    usvg::Tree::from_str(svg, &usvg_options()).map_err(|error| RenderError::Raster(format!("invalid SVG: {error}")))
}

fn rasterize_tree(tree: &usvg::Tree, size: RasterSize) -> Result<Pixmap, RenderError> {
    let svg_size = tree.size();
    let (svg_width, svg_height) = (f64::from(svg_size.width()), f64::from(svg_size.height()));
    let pixels = size.resolve(svg_width, svg_height)?;
    let mut pixmap = Pixmap::new(pixels.width, pixels.height)
        .ok_or_else(|| RenderError::Raster(format!("cannot allocate {}×{} pixmap", pixels.width, pixels.height)))?;
    // Independent x/y factors so the rounded pixel size is filled edge to edge.
    let transform = Transform::from_scale(
        (f64::from(pixels.width) / svg_width) as f32,
        (f64::from(pixels.height) / svg_height) as f32,
    );
    resvg::render(tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED_SQUARE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="20" viewBox="0 0 40 20">
        <rect width="40" height="20" fill="#ffffff"/><rect x="0" y="0" width="20" height="20" fill="#ff0000"/></svg>"##;

    fn pixel(pixmap: &Pixmap, x: u32, y: u32) -> [u8; 4] {
        let color = pixmap.pixel(x, y).map(|pixel| pixel.demultiply());
        color.map_or([0; 4], |c| [c.red(), c.green(), c.blue(), c.alpha()])
    }

    #[test]
    fn scale_multiplies_svg_size() {
        let pixmap = rasterize(RED_SQUARE, RasterSize::Scale(2.0)).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (80, 40));
        assert_eq!(pixel(&pixmap, 10, 20), [255, 0, 0, 255]);
        assert_eq!(pixel(&pixmap, 70, 20), [255, 255, 255, 255]);
    }

    #[test]
    fn width_keeps_aspect_ratio() {
        let pixmap = rasterize(RED_SQUARE, RasterSize::Width(100)).unwrap();
        assert_eq!((pixmap.width(), pixmap.height()), (100, 50));
    }

    #[test]
    fn text_uses_embedded_fonts() {
        let svg = r##"<svg xmlns="http://www.w3.org/2000/svg" width="200" height="40">
            <rect width="200" height="40" fill="#fff"/>
            <text x="4" y="30" font-family="Inter" font-size="28" fill="#000">Hello</text></svg>"##;
        let pixmap = rasterize(svg, RasterSize::Scale(1.0)).unwrap();
        let dark = pixmap.pixels().iter().filter(|pixel| pixel.red() < 100).count();
        assert!(dark > 100, "text was not drawn ({dark} dark pixels)");
    }

    #[test]
    fn oversized_output_is_a_limit_error() {
        let error = rasterize(RED_SQUARE, RasterSize::Width(MAX_SIDE_PX + 1)).unwrap_err();
        assert!(matches!(error, RenderError::Limit { what: "width", .. }), "{error}");
        let error = rasterize(RED_SQUARE, RasterSize::Scale(1000.0)).unwrap_err();
        assert!(matches!(error, RenderError::Limit { .. }), "{error}");
    }

    #[test]
    fn invalid_svg_is_a_raster_error() {
        let error = rasterize("<svg", RasterSize::Scale(1.0)).unwrap_err();
        assert!(matches!(error, RenderError::Raster(ref message) if message.starts_with("invalid SVG")), "{error}");
    }

    #[test]
    fn non_positive_scale_is_a_request_error() {
        assert!(matches!(rasterize(RED_SQUARE, RasterSize::Scale(0.0)), Err(RenderError::Request(_))));
        assert!(matches!(rasterize(RED_SQUARE, RasterSize::Scale(f64::NAN)), Err(RenderError::Request(_))));
        assert!(matches!(rasterize(RED_SQUARE, RasterSize::Width(0)), Err(RenderError::Request(_))));
    }
}
