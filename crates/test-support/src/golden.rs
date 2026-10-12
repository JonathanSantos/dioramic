//! Golden files in `tests/golden/`.
//!
//! - `.svg` (and other text files): exact text comparison; on mismatch a unified diff (long SVG lines are
//!   split at tag boundaries first, so the diff points at the element that changed).
//! - `.png`: per-channel RMS similarity with `image-compare`; fails when `1 − similarity > PNG_TOLERANCE`.
//! - anything else: byte comparison.
//!
//! On a mismatch the actual output is also written to `target/golden-actual/<name>` for inspection.
//! Convention: tests that call [`check`] (or `golden!`) have `golden` in their name, so
//! `cargo xtask golden` can select them.

use std::io::Cursor;
use std::path::{Path, PathBuf};

use image_compare::prelude::GrayImage;
use similar::TextDiff;

/// Environment variable that switches [`check`] to update mode.
pub const UPDATE_ENV: &str = "DIORAMIC_UPDATE_GOLDEN";
/// Maximum tolerated pixel difference for PNG goldens (anti-aliasing noise across platforms).
pub const PNG_TOLERANCE: f64 = 0.002;

/// Lines of unified diff shown before truncating.
const MAX_DIFF_LINES: usize = 120;
/// Context lines around each change in the diff.
const DIFF_CONTEXT_LINES: usize = 2;
/// Lines longer than this are split at `>` before diffing (single-line SVG output).
const LONG_LINE_CHARS: usize = 200;
/// Command that regenerates the goldens.
const UPDATE_COMMAND: &str = "cargo xtask golden --update";

/// Path of a golden file.
pub fn path(name: &str) -> PathBuf {
    crate::workspace_root().join("tests").join("golden").join(name)
}

/// Whether [`check`] rewrites instead of comparing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Compare and panic on mismatch.
    Check,
    /// Write `actual` to the golden file (only when it changed).
    Update,
}

impl Mode {
    /// [`Mode::Update`] when `DIORAMIC_UPDATE_GOLDEN` is set to anything but `0`/empty.
    pub fn from_env() -> Self {
        match std::env::var(UPDATE_ENV) {
            Ok(value) if !value.is_empty() && value != "0" => Self::Update,
            _ => Self::Check,
        }
    }
}

/// See [`crate::golden!`].
pub fn check(name: &str, actual: &[u8]) {
    check_file(&path(name), actual, Mode::from_env());
}

/// [`check`] against an explicit file (used by the tests of this module and by custom golden roots).
pub fn check_file(golden: &Path, actual: &[u8], mode: Mode) {
    match mode {
        Mode::Update => update(golden, actual),
        Mode::Check => compare(golden, actual),
    }
}

fn update(golden: &Path, actual: &[u8]) {
    if std::fs::read(golden).is_ok_and(|expected| expected == actual) {
        return;
    }
    if let Some(parent) = golden.parent() {
        std::fs::create_dir_all(parent).unwrap_or_else(|error| panic!("golden {}: {error}", parent.display()));
    }
    std::fs::write(golden, actual).unwrap_or_else(|error| panic!("golden {}: {error}", golden.display()));
}

fn compare(golden: &Path, actual: &[u8]) {
    let expected = std::fs::read(golden)
        .unwrap_or_else(|_| panic!("golden file {} is missing; run `{UPDATE_COMMAND}` to create it", golden.display()));
    if expected == actual {
        return;
    }
    let mismatch = match FileKind::of(golden) {
        FileKind::Png => png_mismatch(&expected, actual),
        FileKind::Text => Some(text_mismatch(&expected, actual)),
        FileKind::Binary => Some(format!("{} bytes expected, {} bytes actual", expected.len(), actual.len())),
    };
    if let Some(details) = mismatch {
        let saved = save_actual(golden, actual);
        panic!(
            "golden {} differs\n{details}\nactual output: {saved}\nif the change is intended, run `{UPDATE_COMMAND}`",
            golden.display()
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileKind {
    Png,
    Text,
    Binary,
}

impl FileKind {
    fn of(path: &Path) -> Self {
        match path.extension().and_then(|extension| extension.to_str()) {
            Some("png") => Self::Png,
            Some("svg" | "json" | "txt" | "yaml" | "yml" | "md") => Self::Text,
            _ => Self::Binary,
        }
    }
}

fn text_mismatch(expected: &[u8], actual: &[u8]) -> String {
    let expected = diffable_text(&String::from_utf8_lossy(expected));
    let actual = diffable_text(&String::from_utf8_lossy(actual));
    let diff = TextDiff::from_lines(&expected, &actual);
    let unified = diff.unified_diff().context_radius(DIFF_CONTEXT_LINES).header("golden", "actual").to_string();
    let total = unified.lines().count();
    let mut shown: Vec<&str> = unified.lines().take(MAX_DIFF_LINES).collect();
    if total > MAX_DIFF_LINES {
        shown.push("… (diff truncated)");
    }
    shown.join("\n")
}

/// Splits very long lines after each `>` so a one-line SVG still produces a readable line diff.
fn diffable_text(text: &str) -> String {
    let has_long_lines = text.lines().any(|line| line.len() > LONG_LINE_CHARS);
    if has_long_lines { text.replace('>', ">\n") } else { text.to_owned() }
}

/// `None` when the images are similar enough, else a description of the difference.
fn png_mismatch(expected: &[u8], actual: &[u8]) -> Option<String> {
    let expected = match RgbaImage::decode(expected) {
        Ok(image) => image,
        Err(error) => return Some(format!("golden PNG cannot be decoded: {error}")),
    };
    let actual = match RgbaImage::decode(actual) {
        Ok(image) => image,
        Err(error) => return Some(format!("actual PNG cannot be decoded: {error}")),
    };
    if (expected.width, expected.height) != (actual.width, actual.height) {
        return Some(format!(
            "size differs: golden {}×{}, actual {}×{}",
            expected.width, expected.height, actual.width, actual.height
        ));
    }
    let difference = 1.0 - rms_similarity(&expected, &actual);
    (difference > PNG_TOLERANCE).then(|| format!("pixel difference {difference:.5} exceeds tolerance {PNG_TOLERANCE}"))
}

/// Minimum over the R, G, B and A channels of `image-compare`'s RMS similarity (1 = identical).
fn rms_similarity(expected: &RgbaImage, actual: &RgbaImage) -> f64 {
    (0..RgbaImage::CHANNELS)
        .map(|channel| {
            let first = expected.channel(channel);
            let second = actual.channel(channel);
            image_compare::gray_similarity_structure(&image_compare::Algorithm::RootMeanSquared, &first, &second)
                .map_or(0.0, |similarity| similarity.score)
        })
        .fold(1.0, f64::min)
}

/// Decoded 8-bit RGBA pixels.
struct RgbaImage {
    width: u32,
    height: u32,
    pixels: Vec<u8>,
}

impl RgbaImage {
    const CHANNELS: usize = 4;

    fn decode(bytes: &[u8]) -> Result<Self, png::DecodingError> {
        let mut decoder = png::Decoder::new(Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::normalize_to_color8());
        let mut reader = decoder.read_info()?;
        let size = reader.output_buffer_size().unwrap_or_default();
        let mut buffer = vec![0; size];
        let info = reader.next_frame(&mut buffer)?;
        buffer.truncate(info.buffer_size());
        let pixels = to_rgba(&buffer, info.color_type);
        Ok(Self { width: info.width, height: info.height, pixels })
    }

    fn channel(&self, channel: usize) -> GrayImage {
        let values: Vec<u8> = self.pixels.iter().skip(channel).step_by(Self::CHANNELS).copied().collect();
        GrayImage::from_raw(self.width, self.height, values).unwrap_or_else(|| GrayImage::new(self.width, self.height))
    }
}

fn to_rgba(buffer: &[u8], color: png::ColorType) -> Vec<u8> {
    match color {
        png::ColorType::Rgba => buffer.to_vec(),
        png::ColorType::Rgb => buffer.as_chunks::<3>().0.iter().flat_map(|&[r, g, b]| [r, g, b, u8::MAX]).collect(),
        png::ColorType::GrayscaleAlpha => {
            buffer.as_chunks::<2>().0.iter().flat_map(|&[value, alpha]| [value, value, value, alpha]).collect()
        }
        png::ColorType::Grayscale | png::ColorType::Indexed => {
            buffer.iter().flat_map(|&value| [value, value, value, u8::MAX]).collect()
        }
    }
}

/// Writes the actual output next to the build artifacts; returns where (or why it could not).
fn save_actual(golden: &Path, actual: &[u8]) -> String {
    let file_name = golden.file_name().map(PathBuf::from).unwrap_or_else(|| PathBuf::from("golden"));
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| crate::workspace_root().join("target"));
    let destination = target.join("golden-actual").join(file_name);
    let written = destination
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&destination, actual));
    match written {
        Ok(()) => destination.display().to_string(),
        Err(error) => format!("(not saved: {error})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh directory per test under the system temp dir.
    fn scratch(test: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!("dioramic-golden-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn png(width: u32, height: u32, pixel: [u8; 4]) -> Vec<u8> {
        encode_pixels(width, height, &vec![pixel; (width * height) as usize])
    }

    const SVG: &str = "<svg>\n  <rect id=\"a\" width=\"10\"/>\n  <text>hello</text>\n</svg>\n";

    #[test]
    fn golden_svg_update_then_check_passes() {
        let file = scratch("svg-roundtrip").join("nested").join("scene.svg");
        check_file(&file, SVG.as_bytes(), Mode::Update);
        check_file(&file, SVG.as_bytes(), Mode::Check);
    }

    #[test]
    #[should_panic(expected = "+  <text>hellO</text>")]
    fn golden_svg_with_changed_byte_fails_with_diff() {
        let file = scratch("svg-changed").join("scene.svg");
        check_file(&file, SVG.as_bytes(), Mode::Update);
        check_file(&file, SVG.replace("hello", "hellO").as_bytes(), Mode::Check);
    }

    #[test]
    #[should_panic(expected = "cargo xtask golden --update")]
    fn golden_missing_file_explains_how_to_create_it() {
        check_file(&scratch("missing").join("absent.svg"), SVG.as_bytes(), Mode::Check);
    }

    #[test]
    fn golden_png_within_tolerance_passes() {
        let file = scratch("png-close").join("scene.png");
        let mut base = vec![[255_u8, 255, 255, 255]; 100 * 100];
        check_file(&file, &encode_pixels(100, 100, &base), Mode::Update);
        base[0] = [254, 255, 255, 255];
        check_file(&file, &encode_pixels(100, 100, &base), Mode::Check);
    }

    #[test]
    #[should_panic(expected = "exceeds tolerance")]
    fn golden_png_with_different_pixels_fails() {
        let file = scratch("png-far").join("scene.png");
        check_file(&file, &png(32, 32, [255, 255, 255, 255]), Mode::Update);
        check_file(&file, &png(32, 32, [0, 0, 0, 255]), Mode::Check);
    }

    #[test]
    #[should_panic(expected = "size differs")]
    fn golden_png_with_different_size_fails() {
        let file = scratch("png-size").join("scene.png");
        check_file(&file, &png(8, 8, [0, 0, 0, 255]), Mode::Update);
        check_file(&file, &png(9, 8, [0, 0, 0, 255]), Mode::Check);
    }

    #[test]
    fn long_lines_are_split_at_tags() {
        let line = format!("<svg>{}</svg>", "<g/>".repeat(80));
        assert!(diffable_text(&line).lines().count() > 80);
        assert_eq!(diffable_text(SVG), SVG);
    }

    fn encode_pixels(width: u32, height: u32, pixels: &[[u8; 4]]) -> Vec<u8> {
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels.concat()).unwrap();
        writer.finish().unwrap();
        bytes
    }
}
