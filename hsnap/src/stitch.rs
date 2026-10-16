//! Stitches successive frames of a vertically scrolling viewport into one tall image.
//!
//! Like the original tool, this assumes the whole viewport scrolls. Sticky headers,
//! footers or animated regions break the row matching; that is a known limit.

use image::RgbaImage;

#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    /// New rows appended to the canvas.
    Appended(u32),
    /// The frame equals the previous one: the end of the page was reached.
    Unchanged,
    /// The frame does not overlap the previous one enough to be placed.
    NoOverlap,
}

pub struct Stitcher {
    width: u32,
    rows: Vec<u8>,
    height: u32,
    last: RgbaImage,
    last_hashes: Vec<u64>,
}

fn row_hashes(img: &RgbaImage) -> Vec<u64> {
    let stride = img.width() as usize * 4;
    img.as_raw()
        .chunks_exact(stride)
        .map(|row| {
            let mut h = 0xcbf29ce484222325u64; // FNV-1a
            for b in row {
                h ^= *b as u64;
                h = h.wrapping_mul(0x100000001b3);
            }
            h
        })
        .collect()
}

/// How many rows the content moved up between `last` and `new` (0 = identical).
fn find_shift(last: &[u64], new: &[u64]) -> Option<usize> {
    let h = last.len();
    if h != new.len() || h == 0 {
        return None;
    }
    if last == new {
        return Some(0);
    }
    let min_overlap = (h / 5).max(8).min(h - 1);
    let mut best: Option<(f32, usize)> = None;
    for s in 1..=(h - min_overlap) {
        let n = h - s;
        let (a, b) = (&last[s..], &new[..n]);
        // A stretch of identical rows (blank page) says nothing about alignment.
        if !b.windows(2).any(|w| w[0] != w[1]) {
            continue;
        }
        let matches = a.iter().zip(b).filter(|(x, y)| x == y).count();
        let ratio = matches as f32 / n as f32;
        if ratio >= 0.9 && best.is_none_or(|(r, _)| ratio > r) {
            best = Some((ratio, s));
            if matches == n {
                break;
            }
        }
    }
    best.map(|(_, s)| s)
}

impl Stitcher {
    pub fn new(first: RgbaImage) -> Self {
        Self { width: first.width(), rows: first.as_raw().clone(), height: first.height(), last_hashes: row_hashes(&first), last: first }
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn add(&mut self, frame: &RgbaImage) -> Added {
        if frame.dimensions() != self.last.dimensions() {
            return Added::NoOverlap;
        }
        let hashes = row_hashes(frame);
        match find_shift(&self.last_hashes, &hashes) {
            Some(0) => Added::Unchanged,
            Some(s) => {
                let stride = self.width as usize * 4;
                let h = frame.height() as usize;
                self.rows.extend_from_slice(&frame.as_raw()[(h - s) * stride..]);
                self.height += s as u32;
                self.last = frame.clone();
                self.last_hashes = hashes;
                Added::Appended(s as u32)
            }
            None => Added::NoOverlap,
        }
    }

    pub fn finish(self) -> RgbaImage {
        RgbaImage::from_raw(self.width, self.height, self.rows).expect("row buffer matches dimensions")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    /// A tall page whose rows are all distinct.
    fn page(w: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(w, h, |x, y| Rgba([(y * 7 + x) as u8, (y / 3) as u8, (y * 13 % 251) as u8, 255]))
    }

    fn viewport(page: &RgbaImage, top: u32, h: u32) -> RgbaImage {
        image::imageops::crop_imm(page, 0, top, page.width(), h).to_image()
    }

    #[test]
    fn rebuilds_a_scrolled_page() {
        let full = page(6, 300);
        let mut st = Stitcher::new(viewport(&full, 0, 100));
        let mut top = 0;
        loop {
            let next = (top + 70).min(full.height() - 100);
            match st.add(&viewport(&full, next, 100)) {
                Added::Unchanged => break,
                Added::Appended(n) => assert_eq!(n, next - top),
                Added::NoOverlap => panic!("lost overlap at {next}"),
            }
            top = next;
        }
        assert_eq!(st.finish(), full);
    }

    #[test]
    fn identical_frame_means_end_of_page() {
        let full = page(4, 150);
        let mut st = Stitcher::new(viewport(&full, 50, 100));
        assert_eq!(st.add(&viewport(&full, 50, 100)), Added::Unchanged);
        assert_eq!(st.height(), 100);
    }

    #[test]
    fn unrelated_frame_is_rejected() {
        let a = page(4, 100);
        let b = RgbaImage::from_fn(4, 100, |x, y| Rgba([(x * 31 + y * 17) as u8 ^ 0x55, 9, (y * y) as u8, 255]));
        let mut st = Stitcher::new(a);
        assert_eq!(st.add(&b), Added::NoOverlap);
    }

    #[test]
    fn blank_overlap_is_not_trusted() {
        let blank = RgbaImage::from_pixel(4, 100, Rgba([255, 255, 255, 255]));
        let mut other = blank.clone();
        other.put_pixel(0, 99, Rgba([0, 0, 0, 255])); // one different row at the very bottom
        let mut st = Stitcher::new(blank);
        assert_eq!(st.add(&other), Added::NoOverlap);
    }
}
