//! PNG output: 8-bit RGB, no interlacing, filter none, fast compression, mode 0600.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use png::{ColorType, DeflateCompression, Encoder, Filter};

use crate::viewer::image::RgbImage;

/// Encode `image` the way [`write`] does, returning the bytes.
pub fn encode(image: &RgbImage) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    encode_into(&mut bytes, image).ok()?;
    Some(bytes)
}

/// Write `image` to `path`. Returns whether the file could be written.
pub fn write(image: &RgbImage, path: &Path) -> bool {
    let file = match File::create(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    encode_into(BufWriter::new(file), image).is_ok()
}

fn encode_into<W: std::io::Write>(sink: W, image: &RgbImage) -> Result<(), png::EncodingError> {
    let mut encoder = Encoder::new(sink, image.width as u32, image.height as u32);
    encoder.set_color(ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    // libpng's level 1, which the C server uses.
    encoder.set_deflate_compression(DeflateCompression::Level(1));
    encoder.set_filter(Filter::NoFilter);
    let mut writer = encoder.write_header()?;
    writer.write_image_data(&image.data)
}
