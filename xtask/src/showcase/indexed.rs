//! PNG files of the showcase: 256-color indexed PNGs for the thumbnails and the mosaic (flat art keeps its
//! look at a fraction of the size, as the T-410 pictures did), true-color RGBA for the theme picture, and
//! decoding of a published thumbnail kept as it is.

use anyhow::{Context, Result, bail};
use dioramic_render::Pixmap;
use png::{BitDepth, ColorType, Compression, Decoder, Encoder, Transformations};
use quantette::deps::palette::Srgb;
use quantette::{ImageRef, PaletteSize, Pipeline, QuantizeMethod};
use resvg::tiny_skia::ColorU8;

/// Colors of an indexed PNG.
const PALETTE_COLORS: u16 = 256;

/// `pixmap` flattened onto white, quantized to [`PALETTE_COLORS`] colors (k-means in Oklab, no dithering:
/// flat fills stay flat) and encoded as an indexed PNG.
pub fn encode_indexed_png(pixmap: &Pixmap) -> Result<Vec<u8>> {
    let pixels: Vec<Srgb<u8>> = pixmap.pixels().iter().map(|pixel| opaque(pixel.demultiply())).collect();
    let image = ImageRef::new(pixmap.width(), pixmap.height(), pixels.as_slice())
        .map_err(|error| anyhow::anyhow!("image of {}×{}: {error}", pixmap.width(), pixmap.height()))?;
    let size = PaletteSize::try_from(PALETTE_COLORS).map_err(|_| anyhow::anyhow!("palette of {PALETTE_COLORS}"))?;
    let indexed = Pipeline::new()
        .palette_size(size)
        .quantize_method(QuantizeMethod::kmeans())
        .ditherer(None)
        .parallel(true)
        .input_image(image)
        .output_srgb8_indexed_image();
    let palette: Vec<u8> = indexed.palette().iter().flat_map(|color| [color.red, color.green, color.blue]).collect();
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, pixmap.width(), pixmap.height());
    encoder.set_color(ColorType::Indexed);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_palette(palette);
    encoder.set_compression(Compression::High);
    let mut writer = encoder.write_header().context("PNG header")?;
    writer.write_image_data(indexed.indices()).context("PNG data")?;
    writer.finish().context("PNG end")?;
    Ok(bytes)
}

/// `pixmap` as a true-color PNG, as `dioramic render` writes it.
pub fn encode_rgba_png(pixmap: &Pixmap) -> Result<Vec<u8>> {
    dioramic_render::png::encode_png(pixmap).context("PNG")
}

/// A PNG file (indexed or not) as a pixmap.
pub fn decode_png(bytes: &[u8]) -> Result<Pixmap> {
    let mut decoder = Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(Transformations::EXPAND | Transformations::STRIP_16 | Transformations::ALPHA);
    let mut reader = decoder.read_info().context("PNG header")?;
    let mut buffer = vec![0; reader.output_buffer_size().context("PNG size")?];
    let info = reader.next_frame(&mut buffer).context("PNG data")?;
    if info.color_type != ColorType::Rgba || info.bit_depth != BitDepth::Eight {
        bail!("unexpected PNG layout {:?} {:?}", info.color_type, info.bit_depth);
    }
    let mut pixmap = Pixmap::new(info.width, info.height).context("empty PNG")?;
    for (pixel, rgba) in pixmap.pixels_mut().iter_mut().zip(buffer.as_chunks::<4>().0) {
        *pixel = ColorU8::from_rgba(rgba[0], rgba[1], rgba[2], rgba[3]).premultiply();
    }
    Ok(pixmap)
}

/// A straight color flattened onto white.
fn opaque(color: ColorU8) -> Srgb<u8> {
    let alpha = u16::from(color.alpha());
    let over_white = |channel: u8| ((u16::from(channel) * alpha + 255 * (255 - alpha) + 127) / 255) as u8;
    Srgb::new(over_white(color.red()), over_white(color.green()), over_white(color.blue()))
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::Color;

    use super::*;

    fn two_colors() -> Pixmap {
        let mut pixmap = Pixmap::new(4, 2).unwrap_or_else(|| panic!("pixmap"));
        pixmap.fill(Color::from_rgba8(30, 90, 200, 255));
        for x in 0..2 {
            let index = x as usize;
            pixmap.pixels_mut()[index] = ColorU8::from_rgba(240, 240, 240, 255).premultiply();
        }
        pixmap
    }

    #[test]
    fn an_indexed_png_round_trips_its_flat_colors() {
        let source = two_colors();
        let bytes = encode_indexed_png(&source).unwrap_or_else(|error| panic!("{error:#}"));
        let decoded = decode_png(&bytes).unwrap_or_else(|error| panic!("{error:#}"));
        assert_eq!((decoded.width(), decoded.height()), (4, 2));
        for (got, want) in decoded.pixels().iter().zip(source.pixels()) {
            let (got, want) = (got.demultiply(), want.demultiply());
            let close = |a: u8, b: u8| a.abs_diff(b) <= 2;
            assert!(close(got.red(), want.red()) && close(got.green(), want.green()) && close(got.blue(), want.blue()));
        }
    }

    #[test]
    fn indexed_output_is_deterministic() {
        let source = two_colors();
        let first = encode_indexed_png(&source).unwrap_or_else(|error| panic!("{error:#}"));
        assert_eq!(first, encode_indexed_png(&source).unwrap_or_else(|error| panic!("{error:#}")));
    }

    #[test]
    fn translucency_flattens_onto_white() {
        assert_eq!(opaque(ColorU8::from_rgba(0, 0, 0, 0)), Srgb::new(255, 255, 255));
        assert_eq!(opaque(ColorU8::from_rgba(10, 20, 30, 255)), Srgb::new(10, 20, 30));
    }
}
