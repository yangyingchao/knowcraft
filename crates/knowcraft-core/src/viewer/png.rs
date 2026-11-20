//! PNG output: 8-bit RGB, no interlacing, filter none, fast compression, mode 0600.

use std::fs::File;
use std::io::BufWriter;
use std::path::Path;

use png::{ColorType, DeflateCompression, Encoder, Filter};

use crate::viewer::image::RgbImage;

/// Write `image` to `path`. Returns whether the file could be written.
pub fn write(image: &RgbImage, path: &Path) -> bool {
    let file = match File::create(path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    let mut w = BufWriter::new(file);
    let mut encoder = Encoder::new(&mut w, image.width as u32, image.height as u32);
    encoder.set_color(ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    // libpng's level 1, which the C server uses.
    encoder.set_deflate_compression(DeflateCompression::Level(1));
    encoder.set_filter(Filter::NoFilter);
    let mut writer = match encoder.write_header() {
        Ok(w) => w,
        Err(_) => return false,
    };
    writer.write_image_data(&image.data).is_ok()
}
