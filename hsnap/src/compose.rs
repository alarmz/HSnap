//! Pure image helpers for free-hand masks and multi-region composition.

use image::{Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Compose {
    /// Keep each fragment at its screen position; fill the gaps with the background.
    #[default]
    Preserve,
    TileVertical,
    TileHorizontal,
}

impl Compose {
    pub fn as_arg(self) -> &'static str {
        match self {
            Compose::Preserve => "preserve",
            Compose::TileVertical => "vertical",
            Compose::TileHorizontal => "horizontal",
        }
    }
    pub fn from_arg(s: &str) -> Self {
        match s {
            "vertical" => Compose::TileVertical,
            "horizontal" => Compose::TileHorizontal,
            _ => Compose::Preserve,
        }
    }
}

/// Even-odd point-in-polygon test.
fn inside(poly: &[(i32, i32)], x: f32, y: f32) -> bool {
    let mut c = false;
    let n = poly.len();
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = (poly[i].0 as f32, poly[i].1 as f32);
        let (xj, yj) = (poly[j].0 as f32, poly[j].1 as f32);
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
        j = i;
    }
    c
}

/// Crop `shot` to the bounding box of `path` (image pixels); everything outside the
/// polygon is replaced by `bg`. Returns the crop and its top-left corner.
pub fn freehand_crop(shot: &RgbaImage, path: &[(i32, i32)], bg: [u8; 4]) -> Option<(RgbaImage, (i32, i32))> {
    if path.len() < 3 {
        return None;
    }
    let (iw, ih) = (shot.width() as i32, shot.height() as i32);
    let x0 = path.iter().map(|p| p.0).min()?.clamp(0, iw - 1);
    let y0 = path.iter().map(|p| p.1).min()?.clamp(0, ih - 1);
    let x1 = path.iter().map(|p| p.0).max()?.clamp(x0 + 1, iw);
    let y1 = path.iter().map(|p| p.1).max()?.clamp(y0 + 1, ih);
    let mut out = RgbaImage::from_pixel((x1 - x0) as u32, (y1 - y0) as u32, Rgba(bg));
    for y in y0..y1 {
        for x in x0..x1 {
            if inside(path, x as f32 + 0.5, y as f32 + 0.5) {
                out.put_pixel((x - x0) as u32, (y - y0) as u32, *shot.get_pixel(x as u32, y as u32));
            }
        }
    }
    Some((out, (x0, y0)))
}

/// Assemble fragments, each given with its top-left position on the screenshot.
pub fn compose_regions(parts: &[((i32, i32), RgbaImage)], mode: Compose, margin: u32, bg: [u8; 4]) -> Option<RgbaImage> {
    if parts.is_empty() {
        return None;
    }
    match mode {
        Compose::Preserve => {
            let x0 = parts.iter().map(|p| p.0.0).min()?;
            let y0 = parts.iter().map(|p| p.0.1).min()?;
            let x1 = parts.iter().map(|p| p.0.0 + p.1.width() as i32).max()?;
            let y1 = parts.iter().map(|p| p.0.1 + p.1.height() as i32).max()?;
            let mut out = RgbaImage::from_pixel((x1 - x0) as u32, (y1 - y0) as u32, Rgba(bg));
            for ((x, y), img) in parts {
                imageops::replace(&mut out, img, (*x - x0) as i64, (*y - y0) as i64);
            }
            Some(out)
        }
        Compose::TileVertical | Compose::TileHorizontal => {
            let vertical = mode == Compose::TileVertical;
            let gaps = margin * (parts.len() as u32 - 1);
            let (w, h) = if vertical {
                (parts.iter().map(|p| p.1.width()).max()?, parts.iter().map(|p| p.1.height()).sum::<u32>() + gaps)
            } else {
                (parts.iter().map(|p| p.1.width()).sum::<u32>() + gaps, parts.iter().map(|p| p.1.height()).max()?)
            };
            let mut out = RgbaImage::from_pixel(w, h, Rgba(bg));
            let mut pos = 0i64;
            for (_, img) in parts {
                if vertical {
                    imageops::replace(&mut out, img, 0, pos);
                    pos += (img.height() + margin) as i64;
                } else {
                    imageops::replace(&mut out, img, pos, 0);
                    pos += (img.width() + margin) as i64;
                }
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: u8) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([c, c, c, 255]))
    }

    #[test]
    fn freehand_masks_outside_polygon() {
        let shot = solid(20, 20, 10);
        // Triangle with its right angle at the top-left of the box.
        let (img, origin) = freehand_crop(&shot, &[(2, 2), (12, 2), (2, 12)], [255, 0, 0, 255]).unwrap();
        assert_eq!(origin, (2, 2));
        assert_eq!(img.dimensions(), (10, 10));
        assert_eq!(img.get_pixel(1, 1), &Rgba([10, 10, 10, 255])); // inside
        assert_eq!(img.get_pixel(9, 9), &Rgba([255, 0, 0, 255])); // outside the hypotenuse
    }

    #[test]
    fn freehand_needs_three_points() {
        assert!(freehand_crop(&solid(5, 5, 0), &[(0, 0), (4, 4)], [0; 4]).is_none());
    }

    #[test]
    fn preserve_keeps_relative_positions_and_fills_gap() {
        let parts = vec![((10, 10), solid(5, 5, 1)), ((30, 20), solid(5, 5, 2))];
        let out = compose_regions(&parts, Compose::Preserve, 0, [9, 9, 9, 255]).unwrap();
        assert_eq!(out.dimensions(), (25, 15));
        assert_eq!(out.get_pixel(0, 0), &Rgba([1, 1, 1, 255]));
        assert_eq!(out.get_pixel(20, 10), &Rgba([2, 2, 2, 255]));
        assert_eq!(out.get_pixel(10, 2), &Rgba([9, 9, 9, 255]));
    }

    #[test]
    fn tiling_uses_margin() {
        let parts = vec![((0, 0), solid(4, 3, 1)), ((50, 50), solid(6, 2, 2))];
        let v = compose_regions(&parts, Compose::TileVertical, 2, [0, 0, 0, 255]).unwrap();
        assert_eq!(v.dimensions(), (6, 3 + 2 + 2));
        assert_eq!(v.get_pixel(0, 5), &Rgba([2, 2, 2, 255]));
        let h = compose_regions(&parts, Compose::TileHorizontal, 2, [0, 0, 0, 255]).unwrap();
        assert_eq!(h.dimensions(), (4 + 2 + 6, 3));
        assert_eq!(h.get_pixel(6, 0), &Rgba([2, 2, 2, 255]));
    }
}
