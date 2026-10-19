//! Image and colour filters (Image and Color menus). All functions are pure.

use image::{Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// A pixel rectangle: x, y, width, height.
pub type Sel = (u32, u32, u32, u32);

fn clamp8(v: f32) -> u8 {
    v.round().clamp(0.0, 255.0) as u8
}

/// Run a size-preserving filter on the selection only, or on the whole image.
pub fn on_region(img: &RgbaImage, sel: Option<Sel>, f: impl FnOnce(RgbaImage) -> RgbaImage) -> RgbaImage {
    match sel {
        Some((x, y, w, h)) if w > 0 && h > 0 => {
            let part = imageops::crop_imm(img, x, y, w.min(img.width().saturating_sub(x)), h.min(img.height().saturating_sub(y))).to_image();
            let mut out = img.clone();
            imageops::replace(&mut out, &f(part), x as i64, y as i64);
            out
        }
        _ => f(img.clone()),
    }
}

// ---------------------------------------------------------------- Image menu

/// Shear: positive horizontal degrees drag the top edge right, positive vertical degrees drag the
/// left edge down. The opposite edge stays put; revealed corners take `bg`.
pub fn shear(img: &RgbaImage, h_deg: f32, v_deg: f32, bg: [u8; 4]) -> RgbaImage {
    let mut cur = img.clone();
    if h_deg.abs() > 0.01 {
        let t = h_deg.to_radians().tan();
        let (w, h) = cur.dimensions();
        let extra = (t.abs() * (h.saturating_sub(1)) as f32).ceil() as u32;
        let mut out = RgbaImage::from_pixel(w + extra, h, Rgba(bg));
        for y in 0..h {
            let off = if t >= 0.0 { t * (h - 1 - y) as f32 } else { -t * y as f32 }.round() as i64;
            for x in 0..w {
                out.put_pixel((x as i64 + off) as u32, y, *cur.get_pixel(x, y));
            }
        }
        cur = out;
    }
    if v_deg.abs() > 0.01 {
        let t = v_deg.to_radians().tan();
        let (w, h) = cur.dimensions();
        let extra = (t.abs() * (w.saturating_sub(1)) as f32).ceil() as u32;
        let mut out = RgbaImage::from_pixel(w, h + extra, Rgba(bg));
        for x in 0..w {
            let off = if t >= 0.0 { t * x as f32 } else { -t * (w - 1 - x) as f32 }.round() as i64;
            for y in 0..h {
                out.put_pixel(x, (y as i64 + off) as u32, *cur.get_pixel(x, y));
            }
        }
        cur = out;
    }
    cur
}

/// Mosaic: every `tile` x `tile` block becomes its average colour, optionally with a darker grout line.
pub fn mosaic(img: &RgbaImage, tile: u32, grout: bool) -> RgbaImage {
    let tile = tile.max(1);
    let (w, h) = img.dimensions();
    let mut out = img.clone();
    for ty in (0..h).step_by(tile as usize) {
        for tx in (0..w).step_by(tile as usize) {
            let (bw, bh) = (tile.min(w - tx), tile.min(h - ty));
            let mut sum = [0u64; 3];
            for y in ty..ty + bh {
                for x in tx..tx + bw {
                    let p = img.get_pixel(x, y);
                    for c in 0..3 {
                        sum[c] += p[c] as u64;
                    }
                }
            }
            let n = (bw * bh) as u64;
            let avg = [(sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8];
            let line = [(avg[0] as f32 * 0.7) as u8, (avg[1] as f32 * 0.7) as u8, (avg[2] as f32 * 0.7) as u8];
            for y in ty..ty + bh {
                for x in tx..tx + bw {
                    let edge = grout && tile >= 4 && (x == tx + bw - 1 || y == ty + bh - 1);
                    let c = if edge { line } else { avg };
                    out.put_pixel(x, y, Rgba([c[0], c[1], c[2], 255]));
                }
            }
        }
    }
    out
}

fn luma(p: &Rgba<u8>) -> f32 {
    0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32
}

/// Emboss: light from `angle_deg` (0 = from the right, counter-clockwise); `depth` scales the relief.
pub fn emboss(img: &RgbaImage, angle_deg: f32, depth: f32) -> RgbaImage {
    let (w, h) = img.dimensions();
    let (lx, ly) = (angle_deg.to_radians().cos(), -angle_deg.to_radians().sin());
    let g = |x: i64, y: i64| luma(img.get_pixel(x.clamp(0, w as i64 - 1) as u32, y.clamp(0, h as i64 - 1) as u32));
    let mut out = RgbaImage::new(w, h);
    for y in 0..h as i64 {
        for x in 0..w as i64 {
            let (gx, gy) = (g(x + 1, y) - g(x - 1, y), g(x, y + 1) - g(x, y - 1));
            let v = clamp8(128.0 + depth * (gx * lx + gy * ly) / 2.0);
            out.put_pixel(x as u32, y as u32, Rgba([v, v, v, 255]));
        }
    }
    out
}

/// Sharpen (positive `amount`, 0..100) or blur (negative, -100..0).
pub fn sharpen_blur(img: &RgbaImage, amount: f32) -> RgbaImage {
    if amount.abs() < 0.5 {
        return img.clone();
    }
    if amount < 0.0 {
        return imageops::blur(img, (-amount / 20.0).max(0.2));
    }
    let blurred = imageops::blur(img, 1.2);
    let k = amount / 40.0;
    let mut out = img.clone();
    for (o, b) in out.pixels_mut().zip(blurred.pixels()) {
        for c in 0..3 {
            o[c] = clamp8(o[c] as f32 + k * (o[c] as f32 - b[c] as f32));
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowParams {
    /// Distance of the shadow from the object, pixels.
    pub size: u32,
    /// Darkness of the shadow core, 0..100.
    pub shade: u8,
    /// Softness, 0 = sharp .. 100 = very diffuse.
    pub diffusion: u8,
    /// Direction the shadow falls, degrees (0 = right, 90 = down).
    pub direction: f32,
    pub bg: [u8; 4],
    pub auto_add: bool,
}

impl Default for ShadowParams {
    fn default() -> Self {
        Self { size: 8, shade: 70, diffusion: 50, direction: 45.0, bg: [255, 255, 255, 255], auto_add: false }
    }
}

fn blurred_mask(w: u32, h: u32, rect: (i64, i64, u32, u32), sigma: f32) -> image::GrayImage {
    let mut m = image::GrayImage::new(w, h);
    for y in rect.1.max(0)..(rect.1 + rect.3 as i64).min(h as i64) {
        for x in rect.0.max(0)..(rect.0 + rect.2 as i64).min(w as i64) {
            m.put_pixel(x as u32, y as u32, image::Luma([255]));
        }
    }
    if sigma > 0.1 { imageops::blur(&m, sigma) } else { m }
}

/// Drop shadow. Without a selection the canvas grows to make room; with one, the shadow falls on
/// the image around the selection.
pub fn shadow(img: &RgbaImage, sel: Option<Sel>, p: &ShadowParams) -> RgbaImage {
    let (dx, dy) = ((p.size as f32 * p.direction.to_radians().cos()).round() as i64, (p.size as f32 * p.direction.to_radians().sin()).round() as i64);
    let sigma = p.diffusion as f32 / 100.0 * (p.size as f32 / 2.0 + 1.0);
    let darkness = p.shade as f32 / 100.0;
    let blend = |dst: &mut Rgba<u8>, cover: u8| {
        let a = cover as f32 / 255.0 * darkness;
        for c in 0..3 {
            dst[c] = clamp8(dst[c] as f32 * (1.0 - a));
        }
    };
    match sel {
        Some((x, y, w, h)) if w > 0 && h > 0 => {
            let mask = blurred_mask(img.width(), img.height(), (x as i64 + dx, y as i64 + dy, w, h), sigma);
            let mut out = img.clone();
            for (px, py, m) in mask.enumerate_pixels() {
                let inside = px >= x && px < x + w && py >= y && py < y + h;
                if !inside {
                    blend(out.get_pixel_mut(px, py), m[0]);
                }
            }
            out
        }
        _ => {
            let m = p.size as i64 + (3.0 * sigma).ceil() as i64 + 1;
            let (cw, ch) = (img.width() + 2 * m as u32, img.height() + 2 * m as u32);
            let mut out = RgbaImage::from_pixel(cw, ch, Rgba(p.bg));
            let mask = blurred_mask(cw, ch, (m + dx, m + dy, img.width(), img.height()), sigma);
            for (px, py, c) in mask.enumerate_pixels() {
                blend(out.get_pixel_mut(px, py), c[0]);
            }
            imageops::replace(&mut out, img, m, m);
            out
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FrameParams {
    pub total: u32,
    pub color: [u8; 4],
    pub three_d: bool,
    pub outer: u32,
    pub outer_raised: bool,
    pub inner: u32,
    pub inner_raised: bool,
    pub auto_add: bool,
}

impl Default for FrameParams {
    fn default() -> Self {
        Self { total: 8, color: [128, 128, 128, 255], three_d: false, outer: 2, outer_raised: true, inner: 2, inner_raised: false, auto_add: false }
    }
}

fn lighten(c: [u8; 4]) -> Rgba<u8> {
    Rgba([clamp8(c[0] as f32 + (255.0 - c[0] as f32) * 0.65), clamp8(c[1] as f32 + (255.0 - c[1] as f32) * 0.65), clamp8(c[2] as f32 + (255.0 - c[2] as f32) * 0.65), 255])
}

fn darken(c: [u8; 4]) -> Rgba<u8> {
    Rgba([clamp8(c[0] as f32 * 0.45), clamp8(c[1] as f32 * 0.45), clamp8(c[2] as f32 * 0.45), 255])
}

/// Colour of a frame pixel `depth` steps from the outer edge, on the given side.
fn frame_color(p: &FrameParams, depth: u32, top_or_left: bool) -> Rgba<u8> {
    if !p.three_d {
        return Rgba(p.color);
    }
    let band = |raised: bool| if raised == top_or_left { lighten(p.color) } else { darken(p.color) };
    if depth < p.outer {
        band(p.outer_raised)
    } else if depth >= p.total.saturating_sub(p.inner) {
        band(p.inner_raised)
    } else {
        Rgba(p.color)
    }
}

/// Draw the frame ring around `inner` (x, y, w, h) on a canvas; the ring may be clipped by the canvas.
fn draw_ring(out: &mut RgbaImage, inner: (i64, i64, i64, i64), p: &FrameParams) {
    let t = p.total as i64;
    let (x0, y0, w, h) = inner;
    for py in (y0 - t).max(0)..(y0 + h + t).min(out.height() as i64) {
        for px in (x0 - t).max(0)..(x0 + w + t).min(out.width() as i64) {
            let (dl, dr) = (x0 - px, px - (x0 + w - 1));
            let (dt, db) = (y0 - py, py - (y0 + h - 1));
            let dist = dl.max(dr).max(dt).max(db);
            if dist < 1 || dist > t {
                continue; // inside the framed area, or beyond the ring
            }
            let top_or_left = dt == dist || (dl == dist && dt != dist && db != dist) || (dl == dist && dt < dist);
            let depth = (t - dist) as u32;
            out.put_pixel(px as u32, py as u32, frame_color(p, depth, top_or_left));
        }
    }
}

/// Frame around the whole image (canvas grows) or around a selection (drawn into the image).
pub fn frame(img: &RgbaImage, sel: Option<Sel>, p: &FrameParams) -> RgbaImage {
    match sel {
        Some((x, y, w, h)) if w > 0 && h > 0 => {
            let mut out = img.clone();
            draw_ring(&mut out, (x as i64, y as i64, w as i64, h as i64), p);
            out
        }
        _ => {
            let t = p.total;
            let mut out = RgbaImage::from_pixel(img.width() + 2 * t, img.height() + 2 * t, Rgba(p.color));
            imageops::replace(&mut out, img, t as i64, t as i64);
            draw_ring(&mut out, (t as i64, t as i64, img.width() as i64, img.height() as i64), p);
            out
        }
    }
}

// ---------------------------------------------------------------- Color menu

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Palette {
    #[default]
    Optimized,
    StdWindows,
    Netscape,
    BlackWhite,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Dither {
    None,
    #[default]
    FloydSteinberg,
    Ordered,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ColorResParams {
    /// Bits per pixel: 1..=8, 16, 24 or 32.
    pub bpp: u8,
    pub palette: Palette,
    pub dither: Dither,
}

impl Default for ColorResParams {
    fn default() -> Self {
        Self { bpp: 8, palette: Palette::Optimized, dither: Dither::FloydSteinberg }
    }
}

const STD16: [[u8; 3]; 16] = [
    [0, 0, 0], [128, 0, 0], [0, 128, 0], [128, 128, 0], [0, 0, 128], [128, 0, 128], [0, 128, 128], [192, 192, 192],
    [128, 128, 128], [255, 0, 0], [0, 255, 0], [255, 255, 0], [0, 0, 255], [255, 0, 255], [0, 255, 255], [255, 255, 255],
];

/// Median-cut palette of at most `n` colours.
fn median_cut(img: &RgbaImage, n: usize) -> Vec<[u8; 3]> {
    let step = (img.pixels().len() / 150_000).max(1);
    let mut pixels: Vec<[u8; 3]> = img.pixels().step_by(step).map(|p| [p[0], p[1], p[2]]).collect();
    if pixels.is_empty() {
        return vec![[0, 0, 0]];
    }
    let mut boxes: Vec<Vec<[u8; 3]>> = vec![std::mem::take(&mut pixels)];
    while boxes.len() < n {
        // Split the box with the widest colour range.
        let range = |b: &Vec<[u8; 3]>| -> (u32, usize) {
            (0..3)
                .map(|c| {
                    let (lo, hi) = b.iter().fold((255u8, 0u8), |(l, h), p| (l.min(p[c]), h.max(p[c])));
                    ((hi - lo) as u32 * b.len().min(1) as u32, c)
                })
                .max()
                .unwrap_or((0, 0))
        };
        let Some((idx, (r, ch))) = boxes.iter().enumerate().filter(|(_, b)| b.len() > 1).map(|(i, b)| (i, range(b))).max_by_key(|(_, (r, _))| *r) else { break };
        if r == 0 {
            break;
        }
        let mut b = boxes.swap_remove(idx);
        b.sort_by_key(|p| p[ch]);
        let upper = b.split_off(b.len() / 2);
        boxes.push(b);
        boxes.push(upper);
    }
    boxes
        .iter()
        .map(|b| {
            let n = b.len() as u64;
            let s = b.iter().fold([0u64; 3], |mut a, p| {
                for c in 0..3 {
                    a[c] += p[c] as u64;
                }
                a
            });
            [(s[0] / n) as u8, (s[1] / n) as u8, (s[2] / n) as u8]
        })
        .collect()
}

fn build_palette(img: &RgbaImage, p: &ColorResParams) -> Vec<[u8; 3]> {
    let n = 1usize << p.bpp.clamp(1, 8);
    match p.palette {
        Palette::BlackWhite => vec![[0, 0, 0], [255, 255, 255]],
        Palette::Netscape if n >= 216 => (0..216).map(|i| [(i / 36) as u8 * 51, ((i / 6) % 6) as u8 * 51, (i % 6) as u8 * 51]).collect(),
        Palette::StdWindows if n >= 16 => {
            let mut pal = STD16.to_vec();
            if n > 16 {
                pal.extend(median_cut(img, n - 16));
            }
            pal
        }
        _ => median_cut(img, n),
    }
}

fn nearest(pal: &[[u8; 3]], c: [i32; 3]) -> [u8; 3] {
    *pal.iter().min_by_key(|q| (0..3).map(|i| (q[i] as i32 - c[i]).pow(2)).sum::<i32>()).unwrap_or(&[0, 0, 0])
}

/// Reduce the number of colours, optionally dithering the error.
pub fn color_resolution(img: &RgbaImage, p: &ColorResParams) -> RgbaImage {
    if p.bpp >= 24 {
        return img.clone();
    }
    let (w, h) = img.dimensions();
    let pal = if p.bpp == 16 { vec![] } else { build_palette(img, p) };
    let snap = |c: [i32; 3]| -> [u8; 3] {
        if p.bpp == 16 {
            let q = |v: i32, bits: u32| {
                let max = (1 << bits) - 1;
                ((v.clamp(0, 255) * max + 127) / 255 * 255 / max) as u8
            };
            [q(c[0], 5), q(c[1], 6), q(c[2], 5)]
        } else {
            nearest(&pal, c)
        }
    };
    let mut cache: HashMap<[i32; 3], [u8; 3]> = HashMap::new();
    let mut snap_cached = |c: [i32; 3]| *cache.entry(c).or_insert_with(|| snap(c));
    let mut out = img.clone();
    match p.dither {
        Dither::None => {
            for px in out.pixels_mut() {
                let q = snap_cached([px[0] as i32, px[1] as i32, px[2] as i32]);
                *px = Rgba([q[0], q[1], q[2], px[3]]);
            }
        }
        Dither::Ordered => {
            const BAYER: [[i32; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];
            let spread = if p.bpp <= 2 { 128 } else if p.bpp <= 4 { 64 } else { 32 };
            for (x, y, px) in out.enumerate_pixels_mut() {
                let bias = (BAYER[(y % 4) as usize][(x % 4) as usize] * spread) / 16 - spread / 2;
                let q = snap_cached([px[0] as i32 + bias, px[1] as i32 + bias, px[2] as i32 + bias].map(|v| v.clamp(0, 255)));
                *px = Rgba([q[0], q[1], q[2], px[3]]);
            }
        }
        Dither::FloydSteinberg => {
            let mut err = vec![[0f32; 3]; (w * h) as usize];
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) as usize;
                    let px = *img.get_pixel(x, y);
                    let want = [0, 1, 2].map(|c| (px[c] as f32 + err[i][c]).clamp(0.0, 255.0));
                    let q = snap([want[0].round() as i32, want[1].round() as i32, want[2].round() as i32]);
                    out.put_pixel(x, y, Rgba([q[0], q[1], q[2], px[3]]));
                    let e = [0, 1, 2].map(|c| want[c] - q[c] as f32);
                    for (dx, dy, f) in [(1i64, 0i64, 7.0 / 16.0), (-1, 1, 3.0 / 16.0), (0, 1, 5.0 / 16.0), (1, 1, 1.0 / 16.0)] {
                        let (nx, ny) = (x as i64 + dx, y as i64 + dy);
                        if nx >= 0 && ny >= 0 && nx < w as i64 && ny < h as i64 {
                            let j = (ny as u32 * w + nx as u32) as usize;
                            for c in 0..3 {
                                err[j][c] += e[c] * f;
                            }
                        }
                    }
                }
            }
        }
    }
    out
}

/// Newspaper-style black and white halftone with a 45 degree screen of period `cell` pixels.
pub fn halftone(img: &RgbaImage, cell: f32) -> RgbaImage {
    let cell = cell.max(2.0);
    let (s, c) = (std::f32::consts::FRAC_1_SQRT_2, std::f32::consts::FRAC_1_SQRT_2);
    let mut out = img.clone();
    for (x, y, px) in out.enumerate_pixels_mut() {
        let (fx, fy) = (x as f32, y as f32);
        let u = (fx * c + fy * s) / cell * std::f32::consts::TAU;
        let v = (-fx * s + fy * c) / cell * std::f32::consts::TAU;
        let threshold = ((u.cos() + v.cos()) / 4.0 + 0.5) * 255.0;
        let ink = luma(px) < threshold;
        let g = if ink { 0 } else { 255 };
        *px = Rgba([g, g, g, 255]);
    }
    out
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CorrectParams {
    pub brightness: f32,
    pub contrast: f32,
    pub hue: f32,
    pub saturation: f32,
    pub gamma: f32,
}

impl Default for CorrectParams {
    fn default() -> Self {
        Self { brightness: 0.0, contrast: 0.0, hue: 0.0, saturation: 0.0, gamma: 1.0 }
    }
}

fn rgb_to_hsv(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        60.0 * (((g - b) / d) % 6.0)
    } else if mx == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    (if h < 0.0 { h + 360.0 } else { h }, if mx == 0.0 { 0.0 } else { d / mx }, mx)
}

fn hsv_to_rgb(h: f32, s: f32, v: f32) -> (f32, f32, f32) {
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r, g, b) = match (h / 60.0) as u32 % 6 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    (r + m, g + m, b + m)
}

pub fn color_correction(img: &RgbaImage, p: &CorrectParams) -> RgbaImage {
    let k = p.contrast / 100.0;
    let factor = if k >= 0.0 { 1.0 / (1.0 - k * 0.99) } else { 1.0 + k };
    let inv_gamma = 1.0 / p.gamma.max(0.05);
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let mut c = [px[0], px[1], px[2]].map(|v| v as f32 / 255.0);
        for v in c.iter_mut() {
            *v = v.powf(inv_gamma) + p.brightness / 100.0;
            *v = ((*v - 0.5) * factor + 0.5).clamp(0.0, 1.0);
        }
        if p.hue != 0.0 || p.saturation != 0.0 {
            let (h, s, v) = rgb_to_hsv(c[0], c[1], c[2]);
            let (r, g, b) = hsv_to_rgb((h + p.hue).rem_euclid(360.0), (s * (1.0 + p.saturation / 100.0)).clamp(0.0, 1.0), v);
            c = [r, g, b];
        }
        *px = Rgba([clamp8(c[0] * 255.0), clamp8(c[1] * 255.0), clamp8(c[2] * 255.0), px[3]]);
    }
    out
}

/// Swap pure black and pure white; every other colour is left alone.
pub fn invert_black_white(mut img: RgbaImage) -> RgbaImage {
    for px in img.pixels_mut() {
        match [px[0], px[1], px[2]] {
            [0, 0, 0] => *px = Rgba([255, 255, 255, px[3]]),
            [255, 255, 255] => *px = Rgba([0, 0, 0, px[3]]),
            _ => {}
        }
    }
    img
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubstEntry {
    pub old: [u8; 3],
    pub new: [u8; 3],
    /// Two-way: `old` becomes `new` and `new` becomes `old`.
    pub swap: bool,
}

impl Default for SubstEntry {
    fn default() -> Self {
        Self { old: [255, 255, 255], new: [255, 255, 255], swap: false }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SubstTable {
    pub entries: Vec<SubstEntry>,
    /// 0 = exact match, 100 = everything matches.
    pub tolerance: f32,
    pub auto_apply: bool,
}

impl Default for SubstTable {
    fn default() -> Self {
        Self { entries: vec![], tolerance: 0.0, auto_apply: false }
    }
}

pub fn substitute(img: &RgbaImage, table: &SubstTable) -> RgbaImage {
    let limit = table.tolerance / 100.0 * 441.67;
    let near = |a: [u8; 3], b: [u8; 3]| (0..3).map(|i| (a[i] as f32 - b[i] as f32).powi(2)).sum::<f32>().sqrt() <= limit + 0.001;
    let mut out = img.clone();
    for px in out.pixels_mut() {
        let c = [px[0], px[1], px[2]];
        for e in &table.entries {
            if near(c, e.old) {
                *px = Rgba([e.new[0], e.new[1], e.new[2], px[3]]);
                break;
            }
            if e.swap && near(c, e.new) {
                *px = Rgba([e.old[0], e.old[1], e.old[2], px[3]]);
                break;
            }
        }
    }
    out
}

pub fn unique_colors(img: &RgbaImage) -> usize {
    img.pixels().map(|p| [p[0], p[1], p[2]]).collect::<HashSet<_>>().len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid(w: u32, h: u32, c: [u8; 3]) -> RgbaImage {
        RgbaImage::from_pixel(w, h, Rgba([c[0], c[1], c[2], 255]))
    }

    fn noisy(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([(x * 7 + y * 3) as u8, (x * 11 ^ y * 5) as u8, (x * y) as u8, 255]))
    }

    #[test]
    fn shear_grows_the_canvas_and_keeps_the_fixed_edge() {
        let img = solid(10, 11, [1, 2, 3]);
        let s = shear(&img, 45.0, 0.0, [9, 9, 9, 255]); // tan = 1 -> top row shifts 10 right
        assert_eq!(s.dimensions(), (20, 11));
        assert_eq!(s.get_pixel(0, 10), &Rgba([1, 2, 3, 255])); // bottom row stays at the left
        assert_eq!(s.get_pixel(0, 0), &Rgba([9, 9, 9, 255])); // revealed corner is background
        assert_eq!(s.get_pixel(19, 0), &Rgba([1, 2, 3, 255]));
        let v = shear(&img, 0.0, 45.0, [9, 9, 9, 255]);
        assert_eq!(v.dimensions(), (10, 20));
        assert_eq!(shear(&img, 0.0, 0.0, [0; 4]), img);
    }

    #[test]
    fn mosaic_averages_tiles() {
        let mut img = solid(4, 4, [0, 0, 0]);
        for x in 0..4 {
            img.put_pixel(x, 0, Rgba([200, 200, 200, 255]));
            img.put_pixel(x, 1, Rgba([200, 200, 200, 255]));
        }
        let m = mosaic(&img, 4, false);
        assert!(m.pixels().all(|p| *p == Rgba([100, 100, 100, 255])));
        let g = mosaic(&img, 4, true);
        assert_eq!(g.get_pixel(3, 0), &Rgba([70, 70, 70, 255])); // grout is darker
        assert_eq!(g.get_pixel(0, 0), &Rgba([100, 100, 100, 255]));
    }

    #[test]
    fn emboss_flat_is_mid_grey_and_edges_show_relief() {
        assert!(emboss(&solid(5, 5, [90, 10, 200]), 45.0, 3.0).pixels().all(|p| p[0] == 128));
        let mut edge = solid(8, 4, [0, 0, 0]);
        for y in 0..4 {
            for x in 4..8 {
                edge.put_pixel(x, y, Rgba([255, 255, 255, 255]));
            }
        }
        let e = emboss(&edge, 0.0, 1.0);
        assert!(e.get_pixel(4, 2)[0] > 128 || e.get_pixel(3, 2)[0] > 128);
        let flipped = emboss(&edge, 180.0, 1.0);
        assert!(flipped.get_pixel(4, 2)[0] < 128 || flipped.get_pixel(3, 2)[0] < 128);
    }

    #[test]
    fn blur_smooths_and_sharpen_increases_contrast() {
        let n = noisy(32, 32);
        let var = |i: &RgbaImage| {
            let m = i.pixels().map(|p| p[0] as f32).sum::<f32>() / (32.0 * 32.0);
            i.pixels().map(|p| (p[0] as f32 - m).powi(2)).sum::<f32>()
        };
        assert!(var(&sharpen_blur(&n, -60.0)) < var(&n));
        assert!(var(&sharpen_blur(&n, 80.0)) > var(&n));
        assert_eq!(sharpen_blur(&n, 0.0), n);
    }

    #[test]
    fn shadow_darkens_beside_the_object() {
        let img = solid(20, 20, [200, 200, 200]);
        let p = ShadowParams { size: 6, shade: 100, diffusion: 0, direction: 0.0, bg: [255, 255, 255, 255], auto_add: false };
        let s = shadow(&img, None, &p);
        assert!(s.width() > 20 && s.height() > 20);
        let m = (s.width() - 20) / 2;
        assert_eq!(s.get_pixel(m + 2, m + 2), &Rgba([200, 200, 200, 255])); // the image itself is on top
        assert!(s.get_pixel(m + 22, m + 10)[0] < 80); // shadow to the right
        assert_eq!(s.get_pixel(0, 0), &Rgba([255, 255, 255, 255])); // far corner is background
        // With a selection the shadow falls onto the image and the selection stays untouched.
        let sel = shadow(&img, Some((2, 2, 8, 8)), &p);
        assert_eq!(sel.dimensions(), (20, 20));
        assert_eq!(sel.get_pixel(5, 5), &Rgba([200, 200, 200, 255]));
        assert!(sel.get_pixel(12, 5)[0] < 80);
    }

    #[test]
    fn flat_and_3d_frames() {
        let img = solid(10, 10, [0, 0, 255]);
        let flat = frame(&img, None, &FrameParams { total: 4, color: [255, 0, 0, 255], ..Default::default() });
        assert_eq!(flat.dimensions(), (18, 18));
        assert_eq!(flat.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(flat.get_pixel(4, 4), &Rgba([0, 0, 255, 255]));
        let p3 = FrameParams { total: 4, three_d: true, outer: 1, outer_raised: true, inner: 1, inner_raised: true, color: [128, 128, 128, 255], ..Default::default() };
        let f3 = frame(&img, None, &p3);
        let (hi, lo) = (f3.get_pixel(9, 0), f3.get_pixel(9, 17)); // top edge vs bottom edge, outer band raised
        assert!(hi[0] > 128 && lo[0] < 128);
        assert_eq!(f3.get_pixel(9, 2), &Rgba([128, 128, 128, 255])); // flat face between the bands
        // Around a selection: drawn inside the image, selection untouched.
        let sel = frame(&solid(30, 30, [0, 0, 255]), Some((10, 10, 10, 10)), &FrameParams { total: 3, color: [255, 0, 0, 255], ..Default::default() });
        assert_eq!(sel.dimensions(), (30, 30));
        assert_eq!(sel.get_pixel(9, 15), &Rgba([255, 0, 0, 255]));
        assert_eq!(sel.get_pixel(12, 12), &Rgba([0, 0, 255, 255]));
        assert_eq!(sel.get_pixel(5, 5), &Rgba([0, 0, 255, 255]));
    }

    #[test]
    fn colour_resolution_limits_the_palette() {
        let img = noisy(40, 40);
        for (bpp, dither) in [(1u8, Dither::None), (4, Dither::FloydSteinberg), (4, Dither::Ordered), (8, Dither::None)] {
            let out = color_resolution(&img, &ColorResParams { bpp, palette: Palette::Optimized, dither });
            assert!(unique_colors(&out) <= 1usize << bpp, "bpp {bpp} {dither:?}: {}", unique_colors(&out));
        }
        let bw = color_resolution(&img, &ColorResParams { bpp: 8, palette: Palette::BlackWhite, dither: Dither::None });
        assert!(unique_colors(&bw) <= 2);
        let std = color_resolution(&img, &ColorResParams { bpp: 4, palette: Palette::StdWindows, dither: Dither::None });
        assert!(std.pixels().all(|p| STD16.contains(&[p[0], p[1], p[2]])));
        let web = color_resolution(&img, &ColorResParams { bpp: 8, palette: Palette::Netscape, dither: Dither::None });
        assert!(web.pixels().all(|p| p[0] % 51 == 0 && p[1] % 51 == 0 && p[2] % 51 == 0));
        assert_eq!(color_resolution(&img, &ColorResParams { bpp: 24, ..Default::default() }), img);
        let sixteen = color_resolution(&img, &ColorResParams { bpp: 16, palette: Palette::Optimized, dither: Dither::None });
        assert!(unique_colors(&sixteen) < unique_colors(&img));
    }

    #[test]
    fn halftone_is_black_and_white_with_more_ink_in_dark_areas() {
        let dark = halftone(&solid(60, 60, [40, 40, 40]), 6.0);
        let light = halftone(&solid(60, 60, [215, 215, 215]), 6.0);
        assert!(dark.pixels().all(|p| p[0] == 0 || p[0] == 255));
        let ink = |i: &RgbaImage| i.pixels().filter(|p| p[0] == 0).count();
        assert!(ink(&dark) > ink(&light) * 3);
        assert_eq!(ink(&halftone(&solid(20, 20, [255, 255, 255]), 6.0)), 0);
    }

    #[test]
    fn colour_correction_behaves() {
        let img = noisy(16, 16);
        assert_eq!(color_correction(&img, &CorrectParams::default()), img);
        let bright = color_correction(&solid(2, 2, [100, 100, 100]), &CorrectParams { brightness: 20.0, ..Default::default() });
        assert!(bright.get_pixel(0, 0)[0] > 140);
        let grey = color_correction(&solid(2, 2, [200, 50, 50]), &CorrectParams { saturation: -100.0, ..Default::default() });
        let p = grey.get_pixel(0, 0);
        assert!(p[0] == p[1] && p[1] == p[2]);
        let hue = color_correction(&solid(2, 2, [255, 0, 0]), &CorrectParams { hue: 120.0, ..Default::default() });
        assert!(hue.get_pixel(0, 0)[1] > 240 && hue.get_pixel(0, 0)[0] < 15);
        let gamma = color_correction(&solid(2, 2, [64, 64, 64]), &CorrectParams { gamma: 2.0, ..Default::default() });
        assert!(gamma.get_pixel(0, 0)[0] > 64);
    }

    #[test]
    fn invert_black_white_only_touches_pure_colours() {
        let mut img = solid(3, 1, [0, 0, 0]);
        img.put_pixel(1, 0, Rgba([255, 255, 255, 255]));
        img.put_pixel(2, 0, Rgba([10, 10, 10, 255]));
        let out = invert_black_white(img);
        assert_eq!(out.get_pixel(0, 0), &Rgba([255, 255, 255, 255]));
        assert_eq!(out.get_pixel(1, 0), &Rgba([0, 0, 0, 255]));
        assert_eq!(out.get_pixel(2, 0), &Rgba([10, 10, 10, 255]));
    }

    #[test]
    fn substitution_replace_swap_and_tolerance() {
        let mut img = solid(4, 1, [255, 255, 255]);
        img.put_pixel(1, 0, Rgba([0, 0, 255, 255]));
        img.put_pixel(2, 0, Rgba([250, 250, 250, 255])); // near white
        img.put_pixel(3, 0, Rgba([0, 255, 0, 255]));
        let replace = SubstTable { entries: vec![SubstEntry { old: [255, 255, 255], new: [255, 0, 0], swap: false }], ..Default::default() };
        let r = substitute(&img, &replace);
        assert_eq!(r.get_pixel(0, 0), &Rgba([255, 0, 0, 255]));
        assert_eq!(r.get_pixel(1, 0), &Rgba([0, 0, 255, 255])); // untouched
        assert_eq!(r.get_pixel(2, 0), &Rgba([250, 250, 250, 255])); // exact match only at 0%
        let tol = SubstTable { tolerance: 5.0, ..replace.clone() };
        assert_eq!(substitute(&img, &tol).get_pixel(2, 0), &Rgba([255, 0, 0, 255]));
        let swap = SubstTable { entries: vec![SubstEntry { old: [255, 255, 255], new: [0, 0, 255], swap: true }], ..Default::default() };
        let s = substitute(&img, &swap);
        assert_eq!(s.get_pixel(0, 0), &Rgba([0, 0, 255, 255]));
        assert_eq!(s.get_pixel(1, 0), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn counts_unique_colours_and_region_filters() {
        let img = noisy(10, 10);
        assert!(unique_colors(&img) > 20);
        assert_eq!(unique_colors(&solid(5, 5, [1, 2, 3])), 1);
        let out = on_region(&solid(10, 10, [10, 10, 10]), Some((2, 2, 3, 3)), |s| invert_white(s));
        assert_eq!(out.get_pixel(3, 3), &Rgba([245, 245, 245, 255]));
        assert_eq!(out.get_pixel(0, 0), &Rgba([10, 10, 10, 255]));
    }

    fn invert_white(mut i: RgbaImage) -> RgbaImage {
        for p in i.pixels_mut() {
            *p = Rgba([255 - p[0], 255 - p[1], 255 - p[2], 255]);
        }
        i
    }
}
