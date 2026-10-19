//! Saving images with their resolution (dots per inch) and reading it back.

use image::RgbaImage;
use std::fs::File;
use std::io::{BufWriter, Read};
use std::path::Path;

fn per_meter(dpi: f32) -> u32 {
    (dpi / 0.0254).round() as u32
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

/// Save `img`, recording `dpi` in the file where the format has room for it (PNG, JPEG, BMP).
pub fn save(img: &RgbaImage, path: &Path, dpi: (f32, f32)) -> Result<(), String> {
    let e = |e: &dyn std::fmt::Display| e.to_string();
    match ext_of(path).as_str() {
        "png" => {
            let file = BufWriter::new(File::create(path).map_err(|x| e(&x))?);
            let mut enc = png::Encoder::new(file, img.width(), img.height());
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            enc.set_pixel_dims(Some(png::PixelDimensions { xppu: per_meter(dpi.0), yppu: per_meter(dpi.1), unit: png::Unit::Meter }));
            let mut w = enc.write_header().map_err(|x| e(&x))?;
            w.write_image_data(img.as_raw()).map_err(|x| e(&x))
        }
        "jpg" | "jpeg" => {
            let rgb = image::DynamicImage::ImageRgba8(img.clone()).to_rgb8(); // JPEG has no alpha
            let file = BufWriter::new(File::create(path).map_err(|x| e(&x))?);
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(file, 92);
            enc.set_pixel_density(image::codecs::jpeg::PixelDensity::dpi(((dpi.0 + dpi.1) / 2.0).round() as u16));
            enc.encode_image(&rgb).map_err(|x| e(&x))
        }
        "bmp" => {
            image::DynamicImage::ImageRgba8(img.clone()).to_rgb8().save(path).map_err(|x| e(&x))?;
            // BITMAPINFOHEADER keeps pixels-per-metre at bytes 38..46.
            let mut bytes = std::fs::read(path).map_err(|x| e(&x))?;
            if bytes.len() > 46 {
                bytes[38..42].copy_from_slice(&per_meter(dpi.0).to_le_bytes());
                bytes[42..46].copy_from_slice(&per_meter(dpi.1).to_le_bytes());
                std::fs::write(path, bytes).map_err(|x| e(&x))?;
            }
            Ok(())
        }
        _ => img.save(path).map_err(|x| e(&x)),
    }
}

/// The dpi stored in a PNG, JPEG or BMP file, if it has one.
pub fn read_dpi(path: &Path) -> Option<(f32, f32)> {
    let ok = |x: f32, y: f32| (x >= 1.0 && y >= 1.0).then_some((x, y));
    match ext_of(path).as_str() {
        "png" => {
            let dec = png::Decoder::new(std::io::BufReader::new(File::open(path).ok()?));
            let reader = dec.read_info().ok()?;
            let d = reader.info().pixel_dims?;
            if d.unit == png::Unit::Meter { ok(d.xppu as f32 * 0.0254, d.yppu as f32 * 0.0254) } else { None }
        }
        "bmp" => {
            let mut head = [0u8; 46];
            File::open(path).ok()?.read_exact(&mut head).ok()?;
            let (x, y) = (u32::from_le_bytes(head[38..42].try_into().ok()?), u32::from_le_bytes(head[42..46].try_into().ok()?));
            ok(x as f32 * 0.0254, y as f32 * 0.0254)
        }
        "jpg" | "jpeg" => {
            let mut buf = vec![0u8; 64];
            let n = File::open(path).ok()?.read(&mut buf).ok()?;
            buf.truncate(n);
            // JFIF APP0: FF E0 len "JFIF\0" ver(2) units xdens ydens
            let i = buf.windows(5).position(|w| w == b"JFIF\0")?;
            let p = &buf[i + 5..];
            let (units, x, y) = (*p.get(2)?, u16::from_be_bytes([*p.get(3)?, *p.get(4)?]), u16::from_be_bytes([*p.get(5)?, *p.get(6)?]));
            match units {
                1 => ok(x as f32, y as f32),
                2 => ok(x as f32 * 2.54, y as f32 * 2.54),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("hsnap-fileio-{}-{name}", std::process::id()))
    }

    #[test]
    fn dpi_round_trips_through_each_format() {
        let img = RgbaImage::from_fn(16, 12, |x, y| image::Rgba([(x * 10) as u8, (y * 10) as u8, 80, 255]));
        for ext in ["png", "jpg", "bmp"] {
            let p = tmp(&format!("a.{ext}"));
            save(&img, &p, (200.0, 150.0)).unwrap();
            let (x, y) = read_dpi(&p).unwrap_or_else(|| panic!("{ext}: no dpi read"));
            if ext == "jpg" {
                assert!((x - 175.0).abs() < 1.0 && (y - 175.0).abs() < 1.0, "jpeg stores one density: {x} {y}");
            } else {
                assert!((x - 200.0).abs() < 0.5 && (y - 150.0).abs() < 0.5, "{ext}: {x} {y}");
            }
            // The picture itself survives (JPEG is lossy, so only check the size there).
            let back = image::open(&p).unwrap().to_rgba8();
            assert_eq!(back.dimensions(), (16, 12));
            if ext != "jpg" {
                assert_eq!(back.get_pixel(5, 5).0[..3], img.get_pixel(5, 5).0[..3]);
            }
            let _ = std::fs::remove_file(p);
        }
    }

    #[test]
    fn unknown_formats_and_missing_files_give_none() {
        assert!(read_dpi(&tmp("nothing.png")).is_none());
        assert!(read_dpi(Path::new("x.gif")).is_none());
        let p = tmp("plain.gif");
        save(&RgbaImage::new(2, 2), &p, (96.0, 96.0)).unwrap();
        assert!(read_dpi(&p).is_none());
        let _ = std::fs::remove_file(p);
    }
}
