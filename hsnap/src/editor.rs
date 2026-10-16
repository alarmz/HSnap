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
    Ellipse(Pt, Pt),
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
    cur: Snapshot,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
}

impl Document {
    pub fn new(name: impl Into<String>, img: RgbaImage) -> Self {
        Self {
            name: name.into(),
            modified: false,
            cur: Snapshot { img: Arc::new(img), shapes: vec![] },
            undo: vec![],
            redo: vec![],
        }
    }

    pub fn image(&self) -> &RgbaImage {
        &self.cur.img
    }
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

fn load_font() -> Option<FontArc> {
    for p in [r"C:\Windows\Fonts\segoeui.ttf", r"C:\Windows\Fonts\arial.ttf"] {
        if let Ok(bytes) = std::fs::read(p) {
            if let Ok(f) = FontArc::try_from_vec(bytes) {
                return Some(f);
            }
        }
    }
    None
}

fn draw_text(img: &mut RgbaImage, p: Pt, text: &str, size: f32, c: Rgba<u8>) {
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
    fn crop_clamps() {
        let mut d = doc();
        d.crop(15, 5, 100, 100);
        assert_eq!(d.image().dimensions(), (5, 5));
    }
}
