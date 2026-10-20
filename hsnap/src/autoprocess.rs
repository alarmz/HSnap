//! Automatic crop and scale applied to every capture (Capture Settings > Crop & Scale).

use image::{RgbaImage, imageops};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct CropParams {
    pub enabled: bool,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ScaleParams {
    pub enabled: bool,
    /// Width and height are pixel counts; otherwise they are factors of the original size (0.5 = half).
    pub in_pixels: bool,
    pub width: f32,
    pub height: f32,
    /// Derive the height from the width so the picture is not distorted.
    pub keep_aspect: bool,
    /// Smooth the result (Lanczos) instead of copying nearest pixels.
    pub interpolate: bool,
}

impl Default for ScaleParams {
    fn default() -> Self {
        Self { enabled: false, in_pixels: false, width: 1.0, height: 1.0, keep_aspect: true, interpolate: true }
    }
}

/// Cut the configured rectangle out of `img`. The rectangle is clamped to the image; if nothing
/// would be left the image is returned unchanged.
pub fn apply_crop(img: &RgbaImage, p: &CropParams) -> RgbaImage {
    if !p.enabled {
        return img.clone();
    }
    let (iw, ih) = img.dimensions();
    if p.left >= iw || p.top >= ih {
        return img.clone();
    }
    let w = p.width.min(iw - p.left);
    let h = p.height.min(ih - p.top);
    if w == 0 || h == 0 {
        return img.clone();
    }
    imageops::crop_imm(img, p.left, p.top, w, h).to_image()
}

/// Target size for \`p\` on an image of \`orig\` size, never smaller than 1x1.
pub fn scaled_size(orig: (u32, u32), p: &ScaleParams) -> (u32, u32) {
    let (ow, oh) = (orig.0 as f32, orig.1 as f32);
    let (w, h) = if p.in_pixels {
        let w = p.width;
        (w, if p.keep_aspect { w * oh / ow.max(1.0) } else { p.height })
    } else {
        let f = p.width;
        (ow * f, oh * if p.keep_aspect { f } else { p.height })
    };
    ((w.round() as u32).clamp(1, 30_000), (h.round() as u32).clamp(1, 30_000))
}

pub fn apply_scale(img: &RgbaImage, p: &ScaleParams) -> RgbaImage {
    if !p.enabled {
        return img.clone();
    }
    let (w, h) = scaled_size(img.dimensions(), p);
    if (w, h) == img.dimensions() {
        return img.clone();
    }
    imageops::resize(img, w, h, if p.interpolate { imageops::FilterType::Lanczos3 } else { imageops::FilterType::Nearest })
}

/// Crop first, then scale, as the manual's tab order suggests.
pub fn process(img: &RgbaImage, crop: &CropParams, scale: &ScaleParams) -> RgbaImage {
    apply_scale(&apply_crop(img, crop), scale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn img(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([x as u8, y as u8, 0, 255]))
    }

    #[test]
    fn crop_takes_the_configured_rectangle() {
        let p = CropParams { enabled: true, left: 10, top: 5, width: 20, height: 8 };
        let c = apply_crop(&img(100, 50), &p);
        assert_eq!(c.dimensions(), (20, 8));
        assert_eq!(c.get_pixel(0, 0), &Rgba([10, 5, 0, 255]));
    }

    #[test]
    fn crop_clamps_and_never_empties_the_image() {
        let p = CropParams { enabled: true, left: 90, top: 0, width: 50, height: 10 };
        assert_eq!(apply_crop(&img(100, 50), &p).dimensions(), (10, 10));
        let outside = CropParams { enabled: true, left: 500, top: 0, width: 5, height: 5 };
        assert_eq!(apply_crop(&img(100, 50), &outside).dimensions(), (100, 50));
        let empty = CropParams { enabled: true, left: 0, top: 0, width: 0, height: 5 };
        assert_eq!(apply_crop(&img(100, 50), &empty).dimensions(), (100, 50));
        assert_eq!(apply_crop(&img(100, 50), &CropParams { width: 5, height: 5, ..Default::default() }).dimensions(), (100, 50)); // disabled
    }

    #[test]
    fn scale_by_pixels_and_by_factor() {
        let px = ScaleParams { enabled: true, in_pixels: true, width: 50.0, height: 20.0, keep_aspect: false, interpolate: true };
        assert_eq!(scaled_size((100, 40), &px), (50, 20));
        let linked = ScaleParams { keep_aspect: true, ..px.clone() };
        assert_eq!(scaled_size((100, 40), &linked), (50, 20)); // height follows the width: 50 * 40/100
        assert_eq!(scaled_size((100, 60), &linked), (50, 30));
        let half = ScaleParams { enabled: true, in_pixels: false, width: 0.5, height: 0.5, keep_aspect: true, interpolate: false };
        assert_eq!(scaled_size((100, 60), &half), (50, 30));
        let stretch = ScaleParams { enabled: true, in_pixels: false, width: 2.0, height: 0.5, keep_aspect: false, interpolate: false };
        assert_eq!(scaled_size((100, 60), &stretch), (200, 30));
        assert_eq!(scaled_size((10, 10), &ScaleParams { width: 0.001, height: 0.001, in_pixels: false, ..half }), (1, 1)); // floor of 1 pixel
    }

    #[test]
    fn scale_changes_the_picture_and_respects_the_enable_switch() {
        let p = ScaleParams { enabled: true, in_pixels: false, width: 2.0, height: 2.0, keep_aspect: true, interpolate: false };
        let big = apply_scale(&img(10, 6), &p);
        assert_eq!(big.dimensions(), (20, 12));
        assert_eq!(big.get_pixel(3, 3), &Rgba([1, 1, 0, 255])); // nearest-neighbour doubles each pixel
        assert_eq!(apply_scale(&img(10, 6), &ScaleParams { enabled: false, ..p }).dimensions(), (10, 6));
    }

    #[test]
    fn crop_then_scale() {
        let crop = CropParams { enabled: true, left: 0, top: 0, width: 40, height: 20 };
        let scale = ScaleParams { enabled: true, in_pixels: true, width: 20.0, height: 20.0, keep_aspect: true, interpolate: true };
        assert_eq!(process(&img(100, 100), &crop, &scale).dimensions(), (20, 10));
    }
}
