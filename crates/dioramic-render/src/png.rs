//! PNG encoding.

use png::{BitDepth, ColorType, Compression, Encoder};
use resvg::tiny_skia::Pixmap;

use crate::error::RenderError;

/// Encodes a pixmap as PNG (RGBA, deterministic: no timestamps).
pub fn encode_png(pixmap: &Pixmap) -> Result<Vec<u8>, RenderError> {
    encode_rgba(&straight_rgba(pixmap), pixmap.width(), pixmap.height())
}

/// Straight (non-premultiplied) RGBA bytes of a pixmap.
pub(crate) fn straight_rgba(pixmap: &Pixmap) -> Vec<u8> {
    pixmap
        .pixels()
        .iter()
        .flat_map(|pixel| {
            let color = pixel.demultiply();
            [color.red(), color.green(), color.blue(), color.alpha()]
        })
        .collect()
}

fn encode_rgba(rgba: &[u8], width: u32, height: u32) -> Result<Vec<u8>, RenderError> {
    let mut bytes = Vec::new();
    let mut encoder = Encoder::new(&mut bytes, width, height);
    encoder.set_color(ColorType::Rgba);
    encoder.set_depth(BitDepth::Eight);
    encoder.set_compression(Compression::Balanced);
    let mut writer = encoder.write_header().map_err(encode_error)?;
    writer.write_image_data(rgba).map_err(encode_error)?;
    writer.finish().map_err(encode_error)?;
    Ok(bytes)
}

fn encode_error(error: png::EncodingError) -> RenderError {
    RenderError::Encode(format!("PNG: {error}"))
}

#[cfg(test)]
mod tests {
    use resvg::tiny_skia::Color;

    use super::*;

    fn sample() -> Pixmap {
        let mut pixmap = Pixmap::new(3, 2).unwrap();
        pixmap.fill(Color::from_rgba8(10, 20, 30, 255));
        pixmap
    }

    #[test]
    fn round_trips_through_the_png_decoder() {
        let bytes = encode_png(&sample()).unwrap();
        let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        let mut reader = decoder.read_info().unwrap();
        let mut buffer = vec![0; reader.output_buffer_size().unwrap()];
        let info = reader.next_frame(&mut buffer).unwrap();
        assert_eq!((info.width, info.height, info.color_type), (3, 2, ColorType::Rgba));
        assert_eq!(&buffer[..4], &[10, 20, 30, 255]);
    }

    #[test]
    fn output_is_deterministic() {
        assert_eq!(encode_png(&sample()).unwrap(), encode_png(&sample()).unwrap());
    }

    #[test]
    fn translucent_pixels_are_demultiplied() {
        let mut pixmap = Pixmap::new(1, 1).unwrap();
        pixmap.fill(Color::from_rgba8(200, 100, 0, 128));
        let rgba = straight_rgba(&pixmap);
        assert_eq!(rgba[3], 128);
        assert!((i16::from(rgba[0]) - 200).abs() <= 1, "{rgba:?}");
    }
}
