//! Contact sheet: one image per example with every view side by side and readable captions.
//!
//! Layout: one column per rotation, one row per theme × density. The frame and the text are an SVG
//! rasterized with resvg and the embedded fonts (the same text stack as the product); thumbnails are then
//! composited into their slots, so the sheet works even while scene rasterization is still a stub.
//!
//! A view rarely has the slot's aspect ratio, so the thumbnail is centered on the sheet background with a
//! thin frame around the real image only: an opaque letterbox would look like part of the render
//! (a white band beside a `midnight` view reads as an engine bug, `docs/work/reviews/UI-motor.md` item c).

use anyhow::{Context, Result};
use dioramic_render::Pixmap;
use dioramic_render::fonts::usvg_options;
use dioramic_spec::{Density, Rotation, ThemeName};
use resvg::tiny_skia::{PixmapPaint, Transform};
use resvg::usvg::Tree;
use test_support::View;

use super::job::{THUMBNAIL_HEIGHT, THUMBNAIL_WIDTH};

const MARGIN: u32 = 28;
const HEADER_HEIGHT: u32 = 78;
const COLUMN_HEADER_HEIGHT: u32 = 34;
const ROW_HEADER_WIDTH: u32 = 150;
const CAPTION_HEIGHT: u32 = 68;
/// Distance between caption baselines.
const CAPTION_LINE_HEIGHT: u32 = 20;
const GAP: u32 = 18;
/// Characters per line of an error message inside an empty slot.
const ERROR_WRAP_CHARS: usize = 58;
/// Lines of error text that fit in a slot.
const ERROR_MAX_LINES: usize = 12;

const BACKGROUND: &str = "#eef0f4";
/// Frame around a thumbnail, and dashed outline of an empty slot.
const SLOT_STROKE: &str = "#b8c0cc";
/// The frame sits this far outside the image so that the image never covers it.
const FRAME_OFFSET: u32 = 1;
const INK: &str = "#1f2937";
const MUTED: &str = "#6b7280";
const ALARM: &str = "#c62828";
const FONT_FAMILY: &str = dioramic_core::text::LABEL_FAMILY;

/// One slot of the sheet.
#[derive(Debug)]
pub struct SheetCell<'a> {
    /// View rendered in this slot.
    pub view: View,
    /// Thumbnail (already fitted to the slot), if the PNG exists.
    pub thumbnail: Option<&'a Pixmap>,
    /// Caption lines: scene quality, legibility, timing.
    pub caption: [String; 3],
    /// Paint the first caption line in the alarm color (collisions, bad arrows, errors).
    pub alarm: bool,
    /// Paint the second caption line in the alarm color (edges through nodes).
    pub legibility_alarm: bool,
    /// Error shown inside an empty slot.
    pub error: Option<String>,
}

/// Rows (theme × density) and columns (rotation) present in `cells`, in matrix order.
#[derive(Debug, Clone, PartialEq)]
struct Grid {
    rows: Vec<(ThemeName, Density)>,
    columns: Vec<Rotation>,
}

impl Grid {
    fn of(cells: &[SheetCell<'_>]) -> Self {
        let mut rows: Vec<(ThemeName, Density)> = Vec::new();
        let mut columns: Vec<Rotation> = Vec::new();
        for cell in cells {
            let row = (cell.view.theme, cell.view.density);
            if !rows.contains(&row) {
                rows.push(row);
            }
            if !columns.contains(&cell.view.rotation) {
                columns.push(cell.view.rotation);
            }
        }
        columns.sort_by_key(|rotation| rotation.degrees());
        Self { rows, columns }
    }

    fn size(&self) -> (u32, u32) {
        let columns = self.columns.len() as u32;
        let rows = self.rows.len() as u32;
        let width = 2 * MARGIN + ROW_HEADER_WIDTH + columns * THUMBNAIL_WIDTH + columns.saturating_sub(1) * GAP;
        let height = 2 * MARGIN
            + HEADER_HEIGHT
            + COLUMN_HEADER_HEIGHT
            + rows * (THUMBNAIL_HEIGHT + CAPTION_HEIGHT)
            + rows.saturating_sub(1) * GAP;
        (width, height)
    }

    fn column_x(&self, column: usize) -> u32 {
        MARGIN + ROW_HEADER_WIDTH + column as u32 * (THUMBNAIL_WIDTH + GAP)
    }

    fn row_y(&self, row: usize) -> u32 {
        MARGIN + HEADER_HEIGHT + COLUMN_HEADER_HEIGHT + row as u32 * (THUMBNAIL_HEIGHT + CAPTION_HEIGHT + GAP)
    }

    /// Top-left of the slot of `view`.
    fn slot(&self, view: View) -> Option<(u32, u32)> {
        let row = self.rows.iter().position(|row| *row == (view.theme, view.density))?;
        let column = self.columns.iter().position(|rotation| *rotation == view.rotation)?;
        Some((self.column_x(column), self.row_y(row)))
    }
}

/// Draws the sheet.
pub fn contact_sheet(title: &str, subtitle: &str, cells: &[SheetCell<'_>]) -> Result<Pixmap> {
    let grid = Grid::of(cells);
    let svg = frame_svg(&grid, title, subtitle, cells);
    let tree = Tree::from_str(&svg, &usvg_options()).context("contact sheet SVG")?;
    let (width, height) = grid.size();
    let mut sheet = Pixmap::new(width, height).context("contact sheet size")?;
    resvg::render(&tree, Transform::identity(), &mut sheet.as_mut());
    for cell in cells {
        if let (Some(thumbnail), Some(slot)) = (cell.thumbnail, grid.slot(cell.view)) {
            let (left, top) = thumbnail_origin(slot, thumbnail);
            sheet.draw_pixmap(
                left as i32,
                top as i32,
                thumbnail.as_ref(),
                &PixmapPaint::default(),
                Transform::identity(),
                None,
            );
        }
    }
    Ok(sheet)
}

fn frame_svg(grid: &Grid, title: &str, subtitle: &str, cells: &[SheetCell<'_>]) -> String {
    let (width, height) = grid.size();
    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}" font-family="{FONT_FAMILY}"><rect width="{width}" height="{height}" fill="{BACKGROUND}"/>"#
    );
    svg.push_str(&text(MARGIN, MARGIN + 30, 28, 700, INK, "start", title));
    svg.push_str(&text(MARGIN, MARGIN + 58, 16, 400, MUTED, "start", subtitle));
    for (column, rotation) in grid.columns.iter().enumerate() {
        let center = grid.column_x(column) + THUMBNAIL_WIDTH / 2;
        let label = format!("{}°", rotation.degrees());
        svg.push_str(&text(center, MARGIN + HEADER_HEIGHT + 22, 18, 600, INK, "middle", &label));
    }
    for (row, (theme, density)) in grid.rows.iter().enumerate() {
        let middle = grid.row_y(row) + THUMBNAIL_HEIGHT / 2;
        svg.push_str(&text(MARGIN, middle - 4, 18, 700, INK, "start", theme.as_str()));
        svg.push_str(&text(MARGIN, middle + 18, 16, 400, MUTED, "start", density.as_str()));
    }
    for cell in cells {
        if let Some((x, y)) = grid.slot(cell.view) {
            svg.push_str(&slot_svg(x, y, cell));
        }
    }
    svg.push_str("</svg>");
    svg
}

/// Top-left where a thumbnail goes so that it is centered in the slot at `(x, y)`.
fn thumbnail_origin((x, y): (u32, u32), thumbnail: &Pixmap) -> (u32, u32) {
    (
        x + THUMBNAIL_WIDTH.saturating_sub(thumbnail.width()) / 2,
        y + THUMBNAIL_HEIGHT.saturating_sub(thumbnail.height()) / 2,
    )
}

fn slot_svg(x: u32, y: u32, cell: &SheetCell<'_>) -> String {
    let mut svg = match cell.thumbnail {
        Some(thumbnail) => thumbnail_frame(thumbnail_origin((x, y), thumbnail), thumbnail),
        None => empty_slot(x, y, cell.error.as_deref().unwrap_or("no image")),
    };
    let caption_top = y + THUMBNAIL_HEIGHT;
    let colors = [if cell.alarm { ALARM } else { INK }, if cell.legibility_alarm { ALARM } else { INK }, MUTED];
    let styles = [(15, 600), (14, 500), (14, 400)];
    for (index, ((line, color), (size, weight))) in cell.caption.iter().zip(colors).zip(styles).enumerate() {
        let baseline = caption_top + (index as u32 + 1) * CAPTION_LINE_HEIGHT;
        svg.push_str(&text(x + 2, baseline, size, weight, color, "start", line));
    }
    svg
}

/// Hairline just outside the image (no fill: the sheet background shows around it).
fn thumbnail_frame((left, top): (u32, u32), thumbnail: &Pixmap) -> String {
    let x = f64::from(left) - f64::from(FRAME_OFFSET) + 0.5;
    let y = f64::from(top) - f64::from(FRAME_OFFSET) + 0.5;
    let width = thumbnail.width() + 2 * FRAME_OFFSET - 1;
    let height = thumbnail.height() + 2 * FRAME_OFFSET - 1;
    format!(r#"<rect x="{x}" y="{y}" width="{width}" height="{height}" fill="none" stroke="{SLOT_STROKE}"/>"#)
}

/// Dashed outline with the error message (no fill, so it never passes for a white render).
fn empty_slot(x: u32, y: u32, message: &str) -> String {
    let mut svg = format!(
        r#"<rect x="{x}" y="{y}" width="{THUMBNAIL_WIDTH}" height="{THUMBNAIL_HEIGHT}" rx="6" fill="none" stroke="{SLOT_STROKE}" stroke-dasharray="6 4"/>"#
    );
    for (index, line) in wrap(message, ERROR_WRAP_CHARS).iter().take(ERROR_MAX_LINES).enumerate() {
        svg.push_str(&text(x + 14, y + 28 + index as u32 * 20, 14, 400, ALARM, "start", line));
    }
    svg
}

fn text(x: u32, y: u32, size: u32, weight: u32, fill: &str, anchor: &str, content: &str) -> String {
    format!(
        r#"<text x="{x}" y="{y}" font-size="{size}" font-weight="{weight}" fill="{fill}" text-anchor="{anchor}">{}</text>"#,
        escape_xml(content)
    )
}

fn escape_xml(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Greedy word wrap; words longer than `width` are hard-split (paths, ids).
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        for piece in word.chars().collect::<Vec<_>>().chunks(width).map(|chunk| chunk.iter().collect::<String>()) {
            if !current.is_empty() && current.chars().count() + 1 + piece.chars().count() > width {
                lines.push(std::mem::take(&mut current));
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(&piece);
        }
    }
    if !current.is_empty() {
        lines.push(current);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(view: View) -> SheetCell<'static> {
        SheetCell {
            view,
            thumbnail: None,
            caption: ["a".into(), "b".into(), "c".into()],
            alarm: false,
            legibility_alarm: false,
            error: None,
        }
    }

    #[test]
    fn grid_follows_the_matrix_order() {
        let cells: Vec<SheetCell<'_>> = View::gallery_matrix().into_iter().map(cell).collect();
        let grid = Grid::of(&cells);
        assert_eq!(grid.columns, Rotation::ALL.to_vec());
        assert_eq!(grid.rows.len(), 4);
        assert_eq!(grid.rows[0], (ThemeName::Blueprint, Density::Spaced));
    }

    #[test]
    fn wrap_breaks_on_words_and_splits_long_tokens() {
        assert_eq!(wrap("not yet implemented: T-104", 12), ["not yet", "implemented:", "T-104"]);
        assert_eq!(wrap("abcdefghij", 4), ["abcd", "efgh", "ij"]);
    }

    #[test]
    fn xml_is_escaped() {
        assert_eq!(escape_xml("a->b & <c>"), "a-&gt;b &amp; &lt;c&gt;");
    }

    #[test]
    fn sheet_renders_with_embedded_fonts() {
        let cells: Vec<SheetCell<'_>> = View::gallery_matrix().into_iter().take(2).map(cell).collect();
        let sheet = contact_sheet("checkout", "test", &cells).unwrap_or_else(|error| panic!("{error:#}"));
        assert_eq!((sheet.width(), sheet.height()), Grid::of(&cells).size());
        let painted_text = sheet.pixels().iter().any(|pixel| pixel.red() < 64 && pixel.alpha() == 255);
        assert!(painted_text, "title text should be rasterized");
    }

    fn solid(width: u32, height: u32, rgba: [u8; 4]) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap_or_else(|| panic!("pixmap"));
        for pixel in pixmap.data_mut().as_chunks_mut::<4>().0 {
            *pixel = rgba;
        }
        pixmap
    }

    #[test]
    fn letterbox_shows_the_sheet_background_not_white() {
        // A tall dark view (compact ~1.15) in a wide slot (~1.45) leaves bands at both sides.
        let dark = solid(380, THUMBNAIL_HEIGHT, [20, 24, 32, 255]);
        let view = View::default();
        let cells = [SheetCell { thumbnail: Some(&dark), ..cell(view) }];
        let sheet = contact_sheet("midnight", "test", &cells).unwrap_or_else(|error| panic!("{error:#}"));
        let grid = Grid::of(&cells);
        let (x, y) = grid.slot(view).unwrap_or_else(|| panic!("slot"));
        let (left, _) = thumbnail_origin((x, y), &dark);
        let band = sheet.pixel(x + 4, y + THUMBNAIL_HEIGHT / 2).unwrap_or_else(|| panic!("pixel"));
        assert_eq!((band.red(), band.green(), band.blue()), (0xee, 0xf0, 0xf4), "letterbox must be the background");
        let image = sheet.pixel(left + 10, y + THUMBNAIL_HEIGHT / 2).unwrap_or_else(|| panic!("pixel"));
        assert_eq!((image.red(), image.green(), image.blue()), (20, 24, 32));
        let frame = sheet.pixel(left - FRAME_OFFSET, y + THUMBNAIL_HEIGHT / 2).unwrap_or_else(|| panic!("pixel"));
        assert!(frame.red() < 0xee, "a frame line hugs the image");
    }

    #[test]
    fn no_white_pixels_outside_the_images() {
        let cells: Vec<SheetCell<'_>> = View::gallery_matrix().into_iter().take(4).map(cell).collect();
        let sheet = contact_sheet("empty", "test", &cells).unwrap_or_else(|error| panic!("{error:#}"));
        let white =
            sheet.pixels().iter().filter(|pixel| pixel.red() == 255 && pixel.green() == 255 && pixel.blue() == 255);
        assert_eq!(white.count(), 0, "empty slots must not be painted white");
    }
}
