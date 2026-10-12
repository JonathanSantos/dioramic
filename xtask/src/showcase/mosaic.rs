//! The gallery mosaic: a title, then the renders two per row, each fitted into a framed tile with its number in
//! a badge. The frame and the text are an SVG rasterized with resvg and the embedded fonts (the product's text
//! stack); the renders are composited into their tiles, and the badges drawn over them.

use anyhow::{Context, Result};
use dioramic_render::Pixmap;
use dioramic_render::fonts::usvg_options;
use resvg::tiny_skia::{FilterQuality, PixmapPaint, Transform};
use resvg::usvg::Tree;

use crate::gallery::pixels::fit_within;

/// Tiles per row.
const COLUMNS: u32 = 2;
/// Tile size (px).
const TILE_WIDTH: u32 = 1100;
const TILE_HEIGHT: u32 = 720;
/// Room between a tile's frame and the render inside it (px).
const TILE_INSET: u32 = 8;
/// Gap between tiles, outer padding and the height of the title block (px).
const GAP: u32 = 28;
const PADDING: u32 = 48;
const HEADER: u32 = 120;
/// Corner radius of a tile and of a badge (px).
const TILE_RADIUS: u32 = 14;
const BADGE_RADIUS: u32 = 10;
/// Badge box, from the tile's top-left corner (px).
const BADGE_OFFSET: u32 = 14;
const BADGE_WIDTH: u32 = 56;
const BADGE_HEIGHT: u32 = 44;
/// Font sizes (px) and the baselines of the title and the subtitle, from the top padding.
const TITLE_SIZE: u32 = 52;
const SUBTITLE_SIZE: u32 = 28;
const BADGE_SIZE: u32 = 30;
const TITLE_BASELINE: u32 = 50;
const SUBTITLE_BASELINE: u32 = 100;
/// Lifts the badge number from the box's middle line to its optical middle (cap height / 2).
const BADGE_TEXT_DROP: u32 = 11;

const TITLE: &str = "Galeria de system designs";
const SUBTITLE: &str = "tema schematic · do cliente-servidor ao assistente de IA · examples/schematic/";
const BACKGROUND: &str = "#f6f7f9";
const TILE_FILL: &str = "#fafbfc";
const TILE_STROKE: &str = "#d6dbe2";
const TILE_STROKE_WIDTH: u32 = 2;
const INK: &str = "#1c2028";
const MUTED: &str = "#626a78";
const BADGE_INK: &str = "#ffffff";
const FONT_FAMILY: &str = dioramic_core::text::LABEL_FAMILY;

/// One render of the mosaic and its number.
#[derive(Debug)]
pub struct Tile {
    /// Badge text (`07`).
    pub number: String,
    /// The render (any size: fitted into the tile).
    pub image: Pixmap,
}

/// Size (px) of the mosaic of `count` tiles.
fn size(count: u32) -> (u32, u32) {
    let rows = count.div_ceil(COLUMNS);
    let width = 2 * PADDING + COLUMNS * TILE_WIDTH + (COLUMNS - 1) * GAP;
    let height = 2 * PADDING + HEADER + rows * TILE_HEIGHT + rows.saturating_sub(1) * GAP;
    (width, height)
}

/// Top-left corner of tile `index`.
fn tile_origin(index: u32) -> (u32, u32) {
    let (row, column) = (index / COLUMNS, index % COLUMNS);
    (PADDING + column * (TILE_WIDTH + GAP), PADDING + HEADER + row * (TILE_HEIGHT + GAP))
}

/// Draws the mosaic of `tiles`, in order, row by row.
pub fn mosaic(tiles: &[Tile]) -> Result<Pixmap> {
    let count = u32::try_from(tiles.len()).context("too many tiles")?;
    let (width, height) = size(count);
    let mut picture = Pixmap::new(width, height).context("mosaic size")?;
    draw_svg(&mut picture, &frame_svg(count))?;
    for (index, tile) in (0..count).zip(tiles) {
        let fitted = fitted(&tile.image)?;
        let (left, top) = tile_origin(index);
        let x = left + (TILE_WIDTH - fitted.width()) / 2;
        let y = top + (TILE_HEIGHT - fitted.height()) / 2;
        picture.draw_pixmap(x as i32, y as i32, fitted.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }
    draw_svg(&mut picture, &badges_svg(tiles, width, height))?;
    Ok(picture)
}

/// `image` fitted inside a tile: averaged down, or enlarged smoothly when it is smaller (a kept thumbnail).
fn fitted(image: &Pixmap) -> Result<Pixmap> {
    let (room_width, room_height) = (TILE_WIDTH - 2 * TILE_INSET, TILE_HEIGHT - 2 * TILE_INSET);
    let scale =
        (f64::from(room_width) / f64::from(image.width())).min(f64::from(room_height) / f64::from(image.height()));
    if scale <= 1.0 {
        return fit_within(image, room_width, room_height).context("empty render");
    }
    let width = ((f64::from(image.width()) * scale).round() as u32).max(1);
    let height = ((f64::from(image.height()) * scale).round() as u32).max(1);
    let mut enlarged = Pixmap::new(width, height).context("empty render")?;
    let paint = PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() };
    enlarged.draw_pixmap(0, 0, image.as_ref(), &paint, Transform::from_scale(scale as f32, scale as f32), None);
    Ok(enlarged)
}

/// Background, title and the frame of each tile.
fn frame_svg(count: u32) -> String {
    let (width, height) = size(count);
    let mut svg = svg_open(width, height);
    svg.push_str(&format!(r#"<rect width="{width}" height="{height}" fill="{BACKGROUND}"/>"#));
    svg.push_str(&text(PADDING, PADDING + TITLE_BASELINE, TITLE_SIZE, 700, INK, "start", TITLE));
    svg.push_str(&text(PADDING, PADDING + SUBTITLE_BASELINE, SUBTITLE_SIZE, 400, MUTED, "start", SUBTITLE));
    for index in 0..count {
        let (x, y) = tile_origin(index);
        svg.push_str(&format!(
            r#"<rect x="{x}" y="{y}" width="{TILE_WIDTH}" height="{TILE_HEIGHT}" rx="{TILE_RADIUS}" fill="{TILE_FILL}" stroke="{TILE_STROKE}" stroke-width="{TILE_STROKE_WIDTH}"/>"#
        ));
    }
    svg.push_str("</svg>");
    svg
}

/// The numbered badges, on a transparent layer drawn over the renders.
fn badges_svg(tiles: &[Tile], width: u32, height: u32) -> String {
    let mut svg = svg_open(width, height);
    for (index, tile) in (0..).zip(tiles) {
        let (left, top) = tile_origin(index);
        let (x, y) = (left + BADGE_OFFSET, top + BADGE_OFFSET);
        svg.push_str(&format!(
            r#"<rect x="{x}" y="{y}" width="{BADGE_WIDTH}" height="{BADGE_HEIGHT}" rx="{BADGE_RADIUS}" fill="{INK}"/>"#
        ));
        let baseline = y + BADGE_HEIGHT / 2 + BADGE_TEXT_DROP;
        svg.push_str(&text(x + BADGE_WIDTH / 2, baseline, BADGE_SIZE, 700, BADGE_INK, "middle", &tile.number));
    }
    svg.push_str("</svg>");
    svg
}

fn svg_open(width: u32, height: u32) -> String {
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="{FONT_FAMILY}">"#
    )
}

fn text(x: u32, y: u32, size: u32, weight: u32, fill: &str, anchor: &str, content: &str) -> String {
    format!(
        r#"<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{}</text>"#,
        escape(content)
    )
}

fn escape(content: &str) -> String {
    content.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

fn draw_svg(picture: &mut Pixmap, svg: &str) -> Result<()> {
    let tree = Tree::from_str(svg, &usvg_options()).context("mosaic SVG")?;
    resvg::render(&tree, Transform::identity(), &mut picture.as_mut());
    Ok(())
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::Color;

    use super::*;

    fn solid(width: u32, height: u32, color: Color) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap_or_else(|| panic!("pixmap"));
        pixmap.fill(color);
        pixmap
    }

    #[test]
    fn ten_tiles_make_five_rows_of_two() {
        assert_eq!(size(10), (2 * 48 + 2 * 1100 + 28, 2 * 48 + 120 + 5 * 720 + 4 * 28));
        assert_eq!(tile_origin(3), (48 + 1100 + 28, 48 + 120 + 720 + 28));
    }

    #[test]
    fn renders_land_centered_in_their_tiles_under_the_badges() {
        let red = Color::from_rgba8(200, 30, 30, 255);
        let tiles = [
            Tile { number: "01".to_owned(), image: solid(2000, 1000, red) },
            Tile { number: "02".to_owned(), image: solid(300, 300, red) },
        ];
        let picture = mosaic(&tiles).unwrap_or_else(|error| panic!("{error:#}"));
        assert_eq!((picture.width(), picture.height()), size(2));
        let pixel = |x: u32, y: u32| picture.pixel(x, y).unwrap_or_else(|| panic!("pixel {x},{y}"));
        let (left, top) = tile_origin(0);
        let middle = pixel(left + TILE_WIDTH / 2, top + TILE_HEIGHT / 2);
        assert_eq!((middle.red(), middle.green(), middle.blue()), (200, 30, 30), "the render fills the middle");
        let badge = pixel(left + BADGE_OFFSET + 3, top + BADGE_OFFSET + BADGE_HEIGHT / 2);
        assert_eq!((badge.red(), badge.green(), badge.blue()), (0x1c, 0x20, 0x28), "the badge is drawn over it");
        let (left, top) = tile_origin(1);
        let small = pixel(left + TILE_WIDTH / 2, top + TILE_HEIGHT / 2);
        assert_eq!(small.red(), 200, "a small image is enlarged into the tile");
    }
}
