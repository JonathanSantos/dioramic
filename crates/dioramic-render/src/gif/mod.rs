//! High-quality GIF (`docs/07-animacao.md#gif-de-alta-qualidade-e-fluido-medido`):
//! one global palette of 255 colors quantized (`quantette` k-means in Oklab) from sample frames, with a
//! reserved slice for soft ramps, + 1 transparent index; ordered dithering only on soft ramps (shadows,
//! gradients; see `dither`); delta frames against what is on screen (pixels already showing the wanted
//! color, or an interchangeable one → transparent, `dispose = keep`), each frame cropped to the changed
//! rectangle; infinite loop; delay 2 cs (50 fps).
//!
//! Frames are expected to be opaque (the engine always paints the theme background); translucent pixels
//! are flattened onto white because GIF has no partial alpha.

mod dither;
mod palette;

use std::borrow::Cow;

use gif::{DisposalMethod, Encoder, Frame, Repeat};
use rayon::prelude::*;
use resvg::tiny_skia::Pixmap;

pub use self::palette::MAX_PALETTE_COLORS;
use self::palette::{IndexCache, PaletteMapper, build_palette};
use crate::error::RenderError;

/// Fastest delay browsers honor (0–1 cs are clamped to 10 cs, which slows the animation down).
pub const MIN_DELAY_CS: u16 = 2;
/// Rows mapped per parallel task (each with its own color memo).
const STRIPE_ROWS: usize = 32;

/// Encoder knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GifOptions {
    /// Frame delay in centiseconds (2 = 50 fps; never below 2).
    pub delay_cs: u16,
    /// Frames sampled to build the global palette.
    pub palette_samples: usize,
    /// Palette size excluding the transparent index (≤ 255).
    pub max_colors: u16,
    /// Delta frames with transparent index.
    pub delta_frames: bool,
    /// Crop each frame to the changed rectangle.
    pub crop_to_changes: bool,
}

impl Default for GifOptions {
    fn default() -> Self {
        Self { delay_cs: 2, palette_samples: 4, max_colors: 255, delta_frames: true, crop_to_changes: true }
    }
}

/// Encodes frames (all the same size) as a looping GIF.
pub fn encode_gif(frames: &[Pixmap], options: &GifOptions) -> Result<Vec<u8>, RenderError> {
    let canvas = Canvas::of(frames)?;
    let palette = build_palette(frames, options.palette_samples, options.max_colors)?;
    let mapper = PaletteMapper::new(&palette);
    let delay = options.delay_cs.max(MIN_DELAY_CS);
    let mut encoded = indexed_frames(frames, &mapper, canvas, options);
    encoded.par_iter_mut().for_each(|frame| {
        frame.delay = delay;
        frame.make_lzw_pre_encoded();
    });
    write_gif(canvas, &mapper.global_palette_bytes(), &encoded)
}

/// Size shared by every frame, validated against the GIF 16-bit limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Canvas {
    width: u16,
    height: u16,
}

impl Canvas {
    fn of(frames: &[Pixmap]) -> Result<Self, RenderError> {
        let first = frames.first().ok_or_else(|| RenderError::Request("GIF needs at least one frame".to_owned()))?;
        if let Some(odd) =
            frames.iter().find(|frame| frame.width() != first.width() || frame.height() != first.height())
        {
            return Err(RenderError::Request(format!(
                "GIF frames differ in size: {}×{} vs {}×{}",
                first.width(),
                first.height(),
                odd.width(),
                odd.height()
            )));
        }
        let side = |pixels: u32, what: &'static str| {
            u16::try_from(pixels).map_err(|_| RenderError::Limit {
                what,
                actual: u64::from(pixels),
                limit: u64::from(u16::MAX),
            })
        };
        Ok(Self { width: side(first.width(), "width")?, height: side(first.height(), "height")? })
    }

    fn width(self) -> usize {
        usize::from(self.width)
    }

    fn height(self) -> usize {
        usize::from(self.height)
    }
}

/// Indexed GIF frames, still uncompressed. Sequential because each delta depends on what the previous
/// frames left on screen; the per-frame color mapping runs in parallel.
fn indexed_frames(
    frames: &[Pixmap],
    mapper: &PaletteMapper,
    canvas: Canvas,
    options: &GifOptions,
) -> Vec<Frame<'static>> {
    let mut encoded = Vec::with_capacity(frames.len());
    // Wanted indices of the previous frame, reused where its pixels did not change.
    let mut previous: Option<(&Pixmap, Vec<u8>)> = None;
    // Indices currently displayed by a decoder (delta frames only).
    let mut screen: Option<Vec<u8>> = None;
    for frame in frames {
        let wanted =
            map_frame(frame, previous.as_ref().map(|(pixmap, indices)| (*pixmap, indices.as_slice())), mapper, canvas);
        let gif_frame = match screen.as_mut() {
            Some(shown) if options.delta_frames => delta_frame(shown, &wanted, mapper, canvas, options.crop_to_changes),
            _ => {
                if options.delta_frames {
                    screen = Some(wanted.clone());
                }
                frame_for(Region::full(canvas), wanted.clone(), None)
            }
        };
        encoded.push(gif_frame);
        previous = Some((frame, wanted));
    }
    encoded
}

/// Palette index wanted for every pixel of `frame`; pixels equal to `previous` reuse its indices
/// (the dither pattern is positional, so the result is the same and much cheaper).
fn map_frame(frame: &Pixmap, previous: Option<(&Pixmap, &[u8])>, mapper: &PaletteMapper, canvas: Canvas) -> Vec<u8> {
    let width = canvas.width();
    let mut wanted = vec![0_u8; width * canvas.height()];
    wanted.par_chunks_mut(width * STRIPE_ROWS).enumerate().for_each(|(stripe, out)| {
        let mut cache = IndexCache::new(mapper);
        let first = stripe * width * STRIPE_ROWS;
        for (offset, slot) in out.iter_mut().enumerate() {
            let at = first + offset;
            let pixel = frame.pixels()[at];
            *slot = match previous {
                Some((before, indices)) if before.pixels()[at] == pixel => indices[at],
                _ => cache.index(pixel, at % width, at / width),
            };
        }
    });
    wanted
}

/// Pixel rectangle `[x0, x1) × [y0, y1)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Region {
    x0: usize,
    y0: usize,
    x1: usize,
    y1: usize,
}

impl Region {
    fn full(canvas: Canvas) -> Self {
        Self { x0: 0, y0: 0, x1: canvas.width(), y1: canvas.height() }
    }

    fn include(region: Option<Self>, x: usize, y: usize) -> Self {
        match region {
            None => Self { x0: x, y0: y, x1: x + 1, y1: y + 1 },
            Some(r) => Self { x0: r.x0.min(x), y0: r.y0.min(y), x1: r.x1.max(x + 1), y1: r.y1.max(y + 1) },
        }
    }
}

/// Delta frame repainting only pixels whose displayed entry is not interchangeable with the wanted one;
/// `shown` is updated to what a decoder displays afterwards. Comparing against the screen (not the
/// previous source frame) keeps skipped small changes from accumulating into drift.
fn delta_frame(shown: &mut [u8], wanted: &[u8], mapper: &PaletteMapper, canvas: Canvas, crop: bool) -> Frame<'static> {
    let transparent = mapper.transparent_index();
    let needs_paint = |at: usize| !mapper.interchangeable(shown[at], wanted[at]);
    let region = if crop { changed_region(canvas, needs_paint) } else { Some(Region::full(canvas)) };
    let Some(region) = region else {
        // Nothing changed: GIF frames need ≥ 1 pixel, so emit a single transparent one.
        return frame_for(Region { x0: 0, y0: 0, x1: 1, y1: 1 }, vec![transparent], Some(transparent));
    };
    let mut indices = Vec::with_capacity((region.x1 - region.x0) * (region.y1 - region.y0));
    for y in region.y0..region.y1 {
        for x in region.x0..region.x1 {
            let at = y * canvas.width() + x;
            if mapper.interchangeable(shown[at], wanted[at]) {
                indices.push(transparent);
            } else {
                shown[at] = wanted[at];
                indices.push(wanted[at]);
            }
        }
    }
    frame_for(region, indices, Some(transparent))
}

fn changed_region(canvas: Canvas, is_changed: impl Fn(usize) -> bool) -> Option<Region> {
    let mut region = None;
    for y in 0..canvas.height() {
        for x in 0..canvas.width() {
            if is_changed(y * canvas.width() + x) {
                region = Some(Region::include(region, x, y));
            }
        }
    }
    region
}

fn frame_for(region: Region, indices: Vec<u8>, transparent: Option<u8>) -> Frame<'static> {
    // Region sides come from a `Canvas`, so they fit in u16.
    let side = |value: usize| u16::try_from(value).unwrap_or(u16::MAX);
    Frame {
        dispose: DisposalMethod::Keep,
        transparent,
        left: side(region.x0),
        top: side(region.y0),
        width: side(region.x1 - region.x0),
        height: side(region.y1 - region.y0),
        buffer: Cow::Owned(indices),
        ..Frame::default()
    }
}

fn write_gif(canvas: Canvas, palette: &[u8], frames: &[Frame<'static>]) -> Result<Vec<u8>, RenderError> {
    let encode_error = |error: gif::EncodingError| RenderError::Encode(format!("GIF: {error}"));
    let mut bytes = Vec::new();
    {
        let mut encoder = Encoder::new(&mut bytes, canvas.width, canvas.height, palette).map_err(encode_error)?;
        encoder.set_repeat(Repeat::Infinite).map_err(encode_error)?;
        for frame in frames {
            encoder.write_lzw_pre_encoded_frame(frame).map_err(encode_error)?;
        }
        encoder.into_inner().map_err(encode_error)?;
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::{Color, Paint, Rect, Transform};

    use super::*;

    fn solid(width: u32, height: u32, color: Color) -> Pixmap {
        let mut pixmap = Pixmap::new(width, height).unwrap();
        pixmap.fill(color);
        pixmap
    }

    fn with_square(base: &Pixmap, x: f32, y: f32, color: Color) -> Pixmap {
        let mut pixmap = base.clone();
        let mut paint = Paint::default();
        paint.set_color(color);
        pixmap.fill_rect(Rect::from_xywh(x, y, 4.0, 4.0).unwrap(), &paint, Transform::identity(), None);
        pixmap
    }

    struct Decoded {
        width: u16,
        height: u16,
        repeat: Repeat,
        frames: Vec<gif::Frame<'static>>,
    }

    fn decode(bytes: &[u8]) -> Decoded {
        let mut options = gif::DecodeOptions::new();
        options.set_color_output(gif::ColorOutput::Indexed);
        let mut decoder = options.read_info(std::io::Cursor::new(bytes)).unwrap();
        let mut frames = Vec::new();
        while let Some(frame) = decoder.read_next_frame().unwrap() {
            frames.push(frame.clone());
        }
        Decoded { width: decoder.width(), height: decoder.height(), repeat: decoder.repeat(), frames }
    }

    fn moving_square_frames() -> Vec<Pixmap> {
        let background = solid(32, 24, Color::from_rgba8(240, 244, 250, 255));
        (0..5)
            .map(|step| with_square(&background, 2.0 + 5.0 * step as f32, 10.0, Color::from_rgba8(220, 40, 40, 255)))
            .collect()
    }

    #[test]
    fn decodes_with_frame_count_delay_and_infinite_loop() {
        let bytes = encode_gif(&moving_square_frames(), &GifOptions::default()).unwrap();
        let gif = decode(&bytes);
        assert_eq!((gif.width, gif.height), (32, 24));
        assert_eq!(gif.repeat, Repeat::Infinite);
        assert_eq!(gif.frames.len(), 5);
        assert!(gif.frames.iter().all(|frame| frame.delay == 2 && frame.dispose == DisposalMethod::Keep));
        assert_eq!((gif.frames[0].width, gif.frames[0].height, gif.frames[0].transparent), (32, 24, None));
    }

    #[test]
    fn delta_frames_are_cropped_to_the_moving_square() {
        let gif = decode(&encode_gif(&moving_square_frames(), &GifOptions::default()).unwrap());
        let second = &gif.frames[1];
        // Square moved from x=2..6 to x=7..11 at y=10..14.
        assert_eq!((second.left, second.top, second.width, second.height), (2, 10, 9, 4));
        assert!(second.transparent.is_some());
    }

    #[test]
    fn identical_frame_is_a_single_transparent_pixel() {
        let frame = solid(16, 16, Color::from_rgba8(10, 20, 30, 255));
        let gif = decode(&encode_gif(&[frame.clone(), frame], &GifOptions::default()).unwrap());
        let repeat = &gif.frames[1];
        assert_eq!((repeat.width, repeat.height), (1, 1));
        assert_eq!(repeat.transparent, Some(repeat.buffer[0]));
    }

    #[test]
    fn without_delta_every_frame_is_full_and_opaque() {
        let options = GifOptions { delta_frames: false, ..GifOptions::default() };
        let gif = decode(&encode_gif(&moving_square_frames(), &options).unwrap());
        assert!(gif.frames.iter().all(|frame| frame.width == 32 && frame.height == 24 && frame.transparent.is_none()));
    }

    #[test]
    fn uncropped_delta_spans_the_canvas() {
        let options = GifOptions { crop_to_changes: false, ..GifOptions::default() };
        let gif = decode(&encode_gif(&moving_square_frames(), &options).unwrap());
        assert_eq!((gif.frames[1].width, gif.frames[1].height), (32, 24));
        assert!(gif.frames[1].transparent.is_some());
    }

    #[test]
    fn delay_never_goes_below_two_centiseconds() {
        let options = GifOptions { delay_cs: 0, ..GifOptions::default() };
        let gif = decode(&encode_gif(&moving_square_frames(), &options).unwrap());
        assert!(gif.frames.iter().all(|frame| frame.delay == MIN_DELAY_CS));
    }

    #[test]
    fn output_is_deterministic() {
        let frames = moving_square_frames();
        assert_eq!(
            encode_gif(&frames, &GifOptions::default()).unwrap(),
            encode_gif(&frames, &GifOptions::default()).unwrap()
        );
    }

    #[test]
    fn rejects_empty_and_mismatched_frames() {
        assert!(matches!(encode_gif(&[], &GifOptions::default()), Err(RenderError::Request(_))));
        let mismatched = [solid(4, 4, Color::WHITE), solid(5, 4, Color::WHITE)];
        assert!(matches!(encode_gif(&mismatched, &GifOptions::default()), Err(RenderError::Request(_))));
    }
}
