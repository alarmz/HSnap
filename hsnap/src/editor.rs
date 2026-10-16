//! Document model: bitmap + vector annotations, undo/redo, image ops, flattening.

use ab_glyph::{Font, FontArc, PxScale, ScaleFont};
use image::{Rgba, RgbaImage, imageops};
use std::sync::Arc;

pub type Pt = (f32, f32);

#[derive(Clone, Debug, PartialEq)]
pub enum Kind {
    Pencil(Vec<Pt>),
    Line(Pt, Pt),
    Arrow(Pt, Pt),
    Rect(Pt, Pt),
    RoundRect(Pt, Pt),
    Ellipse(Pt, Pt),
    FilledRect(Pt, Pt),
    FilledRoundRect(Pt, Pt),
    FilledEllipse(Pt, Pt),
    Text(Pt, String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub kind: Kind,
    pub color: [u8; 4],
    pub width: f32,
}

#[derive(Clone)]
struct Snapshot {
    img: Arc<RgbaImage>,
    shapes: Vec<Shape>,
}

pub struct Document {
    pub name: String,
    pub modified: bool,
    pub path: Option<std::path::PathBuf>,
    cur: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Document {
    pub fn new(name: impl Into<String>, img: RgbaImage) -> Self {
        Self {
            name: name.into(),
            modified: false,
            path: None,
            cur: Snapshot { img: Arc::new(img), shapes: vec![] },
            undo: vec![],
            redo: vec![],
        }
    }

    pub fn image(&self) -> &RgbaImage {
        &self.cur.img
    }
    #[cfg(test)]
    pub fn shapes(&self) -> &[Shape] {
        &self.cur.shapes
    }
    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }
    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    fn checkpoint(&mut self) {
        self.undo.push(self.cur.clone());
        self.redo.clear();
        self.modified = true;
    }

    pub fn add_shape(&mut self, s: Shape) {
        self.checkpoint();
        self.cur.shapes.push(s);
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.cur, prev));
            self.modified = true;
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.cur, next));
            self.modified = true;
        }
    }

    /// Crop to a pixel rectangle; annotations are moved with the pixels.
    pub fn crop(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let (iw, ih) = self.cur.img.dimensions();
        let (x, y) = (x.min(iw.saturating_sub(1)), y.min(ih.saturating_sub(1)));
        let (w, h) = (w.min(iw - x), h.min(ih - y));
        if w == 0 || h == 0 {
            return;
        }
        let flat = self.flatten();
        self.checkpoint();
        self.cur.img = Arc::new(imageops::crop_imm(&flat, x, y, w, h).to_image());
        self.cur.shapes.clear(); // already baked into the pixels
    }

    pub fn rotate90(&mut self) {
        let flat = self.flatten();
        self.checkpoint();
        self.cur.img = Arc::new(imageops::rotate90(&flat));
        self.cur.shapes.clear();
    }

    pub fn flip_horizontal(&mut self) {
        let flat = self.flatten();
        self.checkpoint();
        self.cur.img = Arc::new(imageops::flip_horizontal(&flat));
        self.cur.shapes.clear();
    }

    /// Replace the bitmap with `f(flattened bitmap)`; one undo step.
    pub fn transform(&mut self, f: impl FnOnce(RgbaImage) -> RgbaImage) {
        let flat = self.flatten();
        self.checkpoint();
        self.cur.img = Arc::new(f(flat));
        self.cur.shapes.clear();
    }

    /// Bitmap with all annotations drawn in.
    pub fn flatten(&self) -> RgbaImage {
        let mut out = (*self.cur.img).clone();
        for s in &self.cur.shapes {
            draw_shape(&mut out, s);
        }
        out
    }
}

fn put(img: &mut RgbaImage, x: i32, y: i32, c: Rgba<u8>) {
    if x >= 0 && y >= 0 && (x as u32) < img.width() && (y as u32) < img.height() {
        img.put_pixel(x as u32, y as u32, c);
    }
}

fn disc(img: &mut RgbaImage, cx: f32, cy: f32, r: f32, c: Rgba<u8>) {
    let r2 = r * r;
    for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            let (dx, dy) = (x as f32 - cx, y as f32 - cy);
            if dx * dx + dy * dy <= r2 {
                put(img, x, y, c);
            }
        }
    }
}

fn line(img: &mut RgbaImage, a: Pt, b: Pt, width: f32, c: Rgba<u8>) {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let steps = dx.abs().max(dy.abs()).ceil().max(1.0) as i32;
    let r = (width / 2.0).max(0.5);
    for i in 0..=steps {
        let t = i as f32 / steps as f32;
        disc(img, a.0 + dx * t, a.1 + dy * t, r, c);
    }
}

fn draw_shape(img: &mut RgbaImage, s: &Shape) {
    let c = Rgba(s.color);
    match &s.kind {
        Kind::Pencil(pts) => {
            for w in pts.windows(2) {
                line(img, w[0], w[1], s.width, c);
            }
            if pts.len() == 1 {
                disc(img, pts[0].0, pts[0].1, s.width / 2.0, c);
            }
        }
        Kind::Line(a, b) => line(img, *a, *b, s.width, c),
        Kind::Arrow(a, b) => {
            line(img, *a, *b, s.width, c);
            let ang = (a.1 - b.1).atan2(a.0 - b.0);
            let len = (s.width * 4.0).max(12.0);
            for da in [0.5f32, -0.5] {
                let t = (b.0 + len * (ang + da).cos(), b.1 + len * (ang + da).sin());
                line(img, *b, t, s.width, c);
            }
        }
        Kind::Rect(a, b) => {
            let (l, r) = (a.0.min(b.0), a.0.max(b.0));
            let (t, bt) = (a.1.min(b.1), a.1.max(b.1));
            line(img, (l, t), (r, t), s.width, c);
            line(img, (r, t), (r, bt), s.width, c);
            line(img, (r, bt), (l, bt), s.width, c);
            line(img, (l, bt), (l, t), s.width, c);
        }
        Kind::RoundRect(a, b) => {
            let path = round_rect_path(*a, *b);
            for w in path.windows(2) {
                line(img, w[0], w[1], s.width, c);
            }
        }
        Kind::FilledRect(a, b) => fill_where(img, *a, *b, c, |_, _| true),
        Kind::FilledRoundRect(a, b) => {
            let (l, t, r, bt) = bounds(*a, *b);
            let rad = ((r - l).min(bt - t) / 5.0).max(1.0);
            fill_where(img, *a, *b, c, |x, y| in_round_rect(x, y, l, t, r, bt, rad));
        }
        Kind::FilledEllipse(a, b) => {
            let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let (rx, ry) = (((a.0 - b.0).abs() / 2.0).max(0.5), ((a.1 - b.1).abs() / 2.0).max(0.5));
            fill_where(img, *a, *b, c, |x, y| ((x - cx) / rx).powi(2) + ((y - cy) / ry).powi(2) <= 1.0);
        }
        Kind::Ellipse(a, b) => {
            let (cx, cy) = ((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0);
            let (rx, ry) = ((a.0 - b.0).abs() / 2.0, (a.1 - b.1).abs() / 2.0);
            let n = ((rx + ry) * 2.0).max(16.0) as i32;
            let pt = |i: i32| {
                let t = i as f32 / n as f32 * std::f32::consts::TAU;
                (cx + rx * t.cos(), cy + ry * t.sin())
            };
            for i in 0..n {
                line(img, pt(i), pt(i + 1), s.width, c);
            }
        }
        Kind::Text(p, text) => draw_text(img, *p, text, s.width.max(1.0) * 6.0, c),
    }
}

fn bounds(a: Pt, b: Pt) -> (f32, f32, f32, f32) {
    (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1))
}

fn fill_where(img: &mut RgbaImage, a: Pt, b: Pt, c: Rgba<u8>, inside: impl Fn(f32, f32) -> bool) {
    let (l, t, r, bt) = bounds(a, b);
    for y in t.floor() as i32..=bt.ceil() as i32 {
        for x in l.floor() as i32..=r.ceil() as i32 {
            if inside(x as f32 + 0.5, y as f32 + 0.5) {
                put(img, x, y, c);
            }
        }
    }
}

fn in_round_rect(x: f32, y: f32, l: f32, t: f32, r: f32, b: f32, rad: f32) -> bool {
    if x < l || x > r || y < t || y > b {
        return false;
    }
    let cx = x.clamp(l + rad, r - rad);
    let cy = y.clamp(t + rad, b - rad);
    (x - cx).powi(2) + (y - cy).powi(2) <= rad * rad
}

fn round_rect_path(a: Pt, b: Pt) -> Vec<Pt> {
    let (l, t, r, bt) = bounds(a, b);
    let rad = ((r - l).min(bt - t) / 5.0).max(1.0);
    let mut v = vec![];
    // Four corner arcs, each sampled in 8 steps, walked clockwise from top-left.
    for (cx, cy, start) in [(l + rad, t + rad, 180.0f32), (r - rad, t + rad, 270.0), (r - rad, bt - rad, 0.0), (l + rad, bt - rad, 90.0)] {
        for i in 0..=8 {
            let ang = (start + i as f32 * 90.0 / 8.0).to_radians();
            v.push((cx + rad * ang.cos(), cy + rad * ang.sin()));
        }
    }
    v.push(v[0]);
    v
}

// ---- raster operations (applied to the flattened bitmap) ----

/// Bucket fill: replace the contiguous region matching the clicked pixel.
pub fn flood_fill(img: &mut RgbaImage, x: u32, y: u32, color: [u8; 4]) {
    if x >= img.width() || y >= img.height() {
        return;
    }
    let target = *img.get_pixel(x, y);
    let new = Rgba(color);
    if target == new {
        return;
    }
    let mut stack = vec![(x, y)];
    while let Some((px, py)) = stack.pop() {
        if *img.get_pixel(px, py) != target {
            continue;
        }
        img.put_pixel(px, py, new);
        if px > 0 { stack.push((px - 1, py)); }
        if py > 0 { stack.push((px, py - 1)); }
        if px + 1 < img.width() { stack.push((px + 1, py)); }
        if py + 1 < img.height() { stack.push((px, py + 1)); }
    }
}

/// Erase along a stroke to the background colour.
pub fn erase_stroke(img: &mut RgbaImage, pts: &[Pt], width: f32, bg: [u8; 4]) {
    for w in pts.windows(2) {
        line(img, w[0], w[1], width, Rgba(bg));
    }
    if pts.len() == 1 {
        disc(img, pts[0].0, pts[0].1, width / 2.0, Rgba(bg));
    }
}

/// Spray paint: random dots inside a disc around each stroke point.
pub fn airbrush(img: &mut RgbaImage, pts: &[Pt], width: f32, color: [u8; 4], seed: &mut u64) {
    let mut rnd = || {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 7;
        *seed ^= *seed << 17;
        (*seed >> 11) as f32 / (1u64 << 53) as f32
    };
    let r = width.max(2.0) * 2.0;
    for p in pts {
        for _ in 0..(r * 1.5) as i32 {
            let (ang, d) = (rnd() * std::f32::consts::TAU, rnd().sqrt() * r);
            put(img, (p.0 + d * ang.cos()) as i32, (p.1 + d * ang.sin()) as i32, Rgba(color));
        }
    }
}

pub fn grayscale(mut img: RgbaImage) -> RgbaImage {
    for p in img.pixels_mut() {
        let g = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) as u8;
        p[0] = g; p[1] = g; p[2] = g;
    }
    img
}

pub fn invert(mut img: RgbaImage) -> RgbaImage {
    for p in img.pixels_mut() {
        p[0] = 255 - p[0]; p[1] = 255 - p[1]; p[2] = 255 - p[2];
    }
    img
}

pub fn black_and_white(mut img: RgbaImage) -> RgbaImage {
    for p in img.pixels_mut() {
        let g = 0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32;
        let v = if g >= 128.0 { 255 } else { 0 };
        p[0] = v; p[1] = v; p[2] = v;
    }
    img
}

/// Remove uniform borders matching the top-left pixel colour.
pub fn auto_trim(img: RgbaImage) -> RgbaImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return img;
    }
    let bg = *img.get_pixel(0, 0);
    let (mut x0, mut y0, mut x1, mut y1) = (w, h, 0, 0);
    for (x, y, p) in img.enumerate_pixels() {
        if *p != bg {
            x0 = x0.min(x); y0 = y0.min(y); x1 = x1.max(x); y1 = y1.max(y);
        }
    }
    if x1 < x0 || y1 < y0 {
        return img; // nothing but background
    }
    imageops::crop_imm(&img, x0, y0, x1 - x0 + 1, y1 - y0 + 1).to_image()
}

pub fn scale(img: RgbaImage, new_w: u32, new_h: u32) -> RgbaImage {
    imageops::resize(&img, new_w.max(1), new_h.max(1), imageops::FilterType::Lanczos3)
}

/// Paste `src` at (x, y), clipping to the destination.
pub fn paste(dst: &mut RgbaImage, src: &RgbaImage, x: i64, y: i64) {
    imageops::replace(dst, src, x, y);
}

/// Fill a pixel rectangle with a colour (Clear / Cut).
pub fn fill_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    for yy in y..(y + h).min(img.height()) {
        for xx in x..(x + w).min(img.width()) {
            img.put_pixel(xx, yy, Rgba(color));
        }
    }
}

/// The UI font used for drawn text and stamps, read once.
fn load_font() -> Option<FontArc> {
    static FONT: std::sync::OnceLock<Option<FontArc>> = std::sync::OnceLock::new();
    FONT.get_or_init(|| {
        for p in [r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\arial.ttf"] {
            if let Ok(bytes) = std::fs::read(p) {
                if let Ok(f) = FontArc::try_from_vec(bytes) {
                    return Some(f);
                }
            }
        }
        None
    })
    .clone()
}

/// Advance width of `text` at pixel size `size`.
pub fn text_width(size: f32, text: &str) -> f32 {
    match load_font() {
        Some(f) => {
            let s = f.as_scaled(PxScale::from(size));
            text.chars().map(|c| s.h_advance(s.glyph_id(c))).sum()
        }
        None => text.chars().count() as f32 * size * 0.5,
    }
}

/// Height of one text line at pixel size `size`.
pub fn line_height(size: f32) -> f32 {
    match load_font() {
        Some(f) => f.as_scaled(PxScale::from(size)).height() * 1.1,
        None => size * 1.3,
    }
}

/// Greedy word wrap to `max_w` pixels; explicit newlines are kept, over-long words are split.
pub fn wrap_text(size: f32, text: &str, max_w: f32) -> Vec<String> {
    let mut lines = vec![];
    for para in text.split('\n') {
        let mut cur = String::new();
        for word in para.split_whitespace() {
            let candidate = if cur.is_empty() { word.to_string() } else { format!("{cur} {word}") };
            if text_width(size, &candidate) <= max_w || cur.is_empty() && candidate.chars().count() <= 1 {
                cur = candidate;
                continue;
            }
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
            }
            // A single word wider than the line is broken by characters.
            let mut piece = String::new();
            for ch in word.chars() {
                piece.push(ch);
                if text_width(size, &piece) > max_w && piece.chars().count() > 1 {
                    let last = piece.pop().unwrap();
                    lines.push(std::mem::take(&mut piece));
                    piece.push(last);
                }
            }
            cur = piece;
        }
        lines.push(cur);
    }
    lines
}

pub fn draw_text(img: &mut RgbaImage, p: Pt, text: &str, size: f32, c: Rgba<u8>) {
    let Some(font) = load_font() else { return };
    let scaled = font.as_scaled(PxScale::from(size));
    let mut x = p.0;
    let y = p.1 + scaled.ascent();
    for ch in text.chars() {
        let mut g = scaled.scaled_glyph(ch);
        g.position = ab_glyph::point(x, y);
        x += scaled.h_advance(g.id);
        if let Some(og) = font.outline_glyph(g) {
            let b = og.px_bounds();
            og.draw(|gx, gy, cov| {
                let (px, py) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                if px >= 0 && py >= 0 && (px as u32) < img.width() && (py as u32) < img.height() {
                    let dst = img.get_pixel_mut(px as u32, py as u32);
                    for i in 0..3 {
                        dst[i] = (c[i] as f32 * cov + dst[i] as f32 * (1.0 - cov)) as u8;
                    }
                }
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc() -> Document {
        Document::new("t", RgbaImage::from_pixel(20, 10, Rgba([255, 255, 255, 255])))
    }

    #[test]
    fn undo_redo_shapes() {
        let mut d = doc();
        d.add_shape(Shape { kind: Kind::Line((0., 0.), (19., 0.)), color: [255, 0, 0, 255], width: 1.0 });
        assert_eq!(d.shapes().len(), 1);
        d.undo();
        assert!(d.shapes().is_empty() && d.can_redo());
        d.redo();
        assert_eq!(d.shapes().len(), 1);
    }

    #[test]
    fn flatten_draws_pixels() {
        let mut d = doc();
        d.add_shape(Shape { kind: Kind::Rect((2., 2.), (10., 8.)), color: [0, 0, 255, 255], width: 1.0 });
        let img = d.flatten();
        assert_eq!(img.get_pixel(2, 5), &Rgba([0, 0, 255, 255]));
        assert_eq!(img.get_pixel(6, 5), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn crop_bakes_annotations_and_is_undoable() {
        let mut d = doc();
        d.add_shape(Shape { kind: Kind::Line((0., 5.), (19., 5.)), color: [0, 0, 0, 255], width: 1.0 });
        d.crop(5, 2, 10, 6);
        assert_eq!(d.image().dimensions(), (10, 6));
        assert!(d.shapes().is_empty());
        assert_eq!(d.image().get_pixel(3, 3), &Rgba([0, 0, 0, 255]));
        d.undo();
        assert_eq!(d.image().dimensions(), (20, 10));
        assert_eq!(d.shapes().len(), 1);
    }

    #[test]
    fn flood_fill_stays_inside_boundary() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255]));
        for y in 0..10 {
            img.put_pixel(5, y, Rgba([0, 0, 0, 255]));
        }
        flood_fill(&mut img, 1, 1, [255, 0, 0, 255]);
        assert_eq!(img.get_pixel(0, 9), &Rgba([255, 0, 0, 255]));
        assert_eq!(img.get_pixel(7, 3), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn auto_trim_removes_border() {
        let mut img = RgbaImage::from_pixel(10, 10, Rgba([255, 255, 255, 255]));
        img.put_pixel(3, 4, Rgba([0, 0, 0, 255]));
        img.put_pixel(6, 7, Rgba([0, 0, 0, 255]));
        assert_eq!(auto_trim(img).dimensions(), (4, 4));
    }

    #[test]
    fn colour_ops() {
        let img = RgbaImage::from_pixel(2, 2, Rgba([200, 100, 0, 255]));
        assert_eq!(invert(img.clone()).get_pixel(0, 0), &Rgba([55, 155, 255, 255]));
        let g = grayscale(img.clone());
        let p = g.get_pixel(0, 0);
        assert!(p[0] == p[1] && p[1] == p[2]);
        assert_eq!(black_and_white(RgbaImage::from_pixel(2, 2, Rgba([250, 200, 0, 255]))).get_pixel(0, 0), &Rgba([255, 255, 255, 255]));
        assert_eq!(black_and_white(img).get_pixel(0, 0), &Rgba([0, 0, 0, 255]));
    }

    #[test]
    fn filled_shapes_cover_interior_and_round_rect_clips_corner() {
        let mut d = Document::new("t", RgbaImage::from_pixel(40, 40, Rgba([255, 255, 255, 255])));
        d.add_shape(Shape { kind: Kind::FilledRect((5., 5.), (15., 15.)), color: [0, 0, 0, 255], width: 1.0 });
        d.add_shape(Shape { kind: Kind::FilledRoundRect((20., 20.), (38., 38.)), color: [0, 0, 0, 255], width: 1.0 });
        let img = d.flatten();
        assert_eq!(img.get_pixel(10, 10), &Rgba([0, 0, 0, 255]));
        assert_eq!(img.get_pixel(29, 29), &Rgba([0, 0, 0, 255]));
        assert_eq!(img.get_pixel(20, 20), &Rgba([255, 255, 255, 255])); // rounded corner
    }

    #[test]
    fn crop_clamps() {
        let mut d = doc();
        d.crop(15, 5, 100, 100);
        assert_eq!(d.image().dimensions(), (5, 5));
    }
}
