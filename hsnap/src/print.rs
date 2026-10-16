//! Printing: page layout (pure, shared with the preview) and GDI output.

use image::RgbaImage;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Fit {
    /// Use `scale_percent` as given.
    #[default]
    Scale,
    /// Scale to fill the area between the margins, keeping proportions.
    FillPage,
    /// Scale to the page width; may continue over several pages.
    PageWidth,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Justify {
    #[default]
    Left,
    Center,
    Right,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Paper {
    A4,
    Letter,
    Legal,
    A3,
}

impl Paper {
    pub const ALL: [Paper; 4] = [Paper::A4, Paper::Letter, Paper::Legal, Paper::A3];
    pub fn name(self) -> &'static str {
        match self {
            Paper::A4 => "A4",
            Paper::Letter => "Letter",
            Paper::Legal => "Legal",
            Paper::A3 => "A3",
        }
    }
    /// Portrait size in inches (width, height).
    pub fn inches(self) -> (f32, f32) {
        match self {
            Paper::A4 => (8.268, 11.693),
            Paper::Letter => (8.5, 11.0),
            Paper::Legal => (8.5, 14.0),
            Paper::A3 => (11.693, 16.535),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PageSetup {
    pub paper: Paper,
    pub landscape: bool,
    /// Margins in inches: left, top, right, bottom.
    pub margins: [f32; 4],
    pub best_fit: bool,
    pub center: bool,
    pub fit: Fit,
    pub scale_percent: f32,
    pub auto_fit_if_too_large: bool,
    pub invert: bool,
    pub black_and_white: bool,
    pub draw_frame: bool,
    pub header: String,
    pub footer: String,
    pub header_justify: Justify,
    pub footer_justify: Justify,
    pub font_pt: f32,
}

impl Default for PageSetup {
    fn default() -> Self {
        Self {
            paper: Paper::A4,
            landscape: false,
            margins: [0.75, 0.75, 0.75, 0.75],
            best_fit: true,
            center: true,
            fit: Fit::FillPage,
            scale_percent: 100.0,
            auto_fit_if_too_large: true,
            invert: false,
            black_and_white: false,
            draw_frame: false,
            header: String::new(),
            footer: String::new(),
            header_justify: Justify::Left,
            footer_justify: Justify::Left,
            font_pt: 10.0,
        }
    }
}

/// One printed page. All lengths are in the units given to `layout` (pixels of the
/// target device or points of the preview); `src_*` are image pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct PagePlacement {
    pub src_y: u32,
    pub src_h: u32,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// Orientation to use: best fit turns a wide image to landscape.
pub fn landscape_for(setup: &PageSetup, img_w: u32, img_h: u32) -> bool {
    if setup.best_fit { img_w > img_h } else { setup.landscape }
}

/// Lay an `img_w` x `img_h` image onto pages.
///
/// `page` is the full page size and `margins` are left/top/right/bottom, all in the same
/// units; `scale_100` is how many of those units one image pixel covers at 100%.
/// `reserved_top` / `reserved_bottom` shrink the usable area for header and footer text.
pub fn layout(img_w: u32, img_h: u32, page: (f32, f32), margins: [f32; 4], reserved_top: f32, reserved_bottom: f32, setup: &PageSetup, scale_100: f32) -> Vec<PagePlacement> {
    let area_x = margins[0];
    let area_y = margins[1] + reserved_top;
    let area_w = (page.0 - margins[0] - margins[2]).max(1.0);
    let area_h = (page.1 - margins[1] - margins[3] - reserved_top - reserved_bottom).max(1.0);
    let (iw, ih) = (img_w as f32, img_h as f32);
    let fill = (area_w / iw).min(area_h / ih);
    let scale = match setup.fit {
        Fit::FillPage => fill,
        Fit::PageWidth => area_w / iw,
        Fit::Scale => {
            let s = setup.scale_percent / 100.0 * scale_100;
            if setup.auto_fit_if_too_large && (iw * s > area_w || ih * s > area_h) { fill } else { s }
        }
    };
    let w = iw * scale;
    let x = if setup.center { area_x + (area_w - w).max(0.0) / 2.0 } else { area_x };

    // Rows of the image that fit on one page.
    let rows_per_page = ((area_h / scale).floor() as u32).max(1);
    let mut pages = vec![];
    let mut y0 = 0;
    while y0 < img_h {
        let rows = rows_per_page.min(img_h - y0);
        let h = rows as f32 * scale;
        let y = if setup.center && img_h <= rows_per_page { area_y + (area_h - h) / 2.0 } else { area_y };
        pages.push(PagePlacement { src_y: y0, src_h: rows, x, y, w, h });
        y0 += rows;
    }
    pages
}

/// Apply the "Colors" options of page setup.
pub fn prepare_image(img: &RgbaImage, setup: &PageSetup) -> RgbaImage {
    let mut out = img.clone();
    if setup.black_and_white {
        out = crate::editor::black_and_white(out);
    }
    if setup.invert {
        out = crate::editor::invert(out);
    }
    out
}

/// Expand the %page% / %pages% macros of header and footer text.
pub fn expand_text(t: &str, page: usize, pages: usize, name: &str) -> String {
    t.replace("%page%", &page.to_string()).replace("%pages%", &pages.to_string()).replace("%file%", name)
}

/// Page size (points) and placements for the preview; 1 pixel = 0.75 pt at 96 dpi.
pub fn plan_preview(setup: &PageSetup, img_w: u32, img_h: u32) -> ((f32, f32), Vec<PagePlacement>) {
    let (pw, ph) = setup.paper.inches();
    let page = if landscape_for(setup, img_w, img_h) { (ph * 72.0, pw * 72.0) } else { (pw * 72.0, ph * 72.0) };
    let m = setup.margins.map(|v| v * 72.0);
    let reserve = |t: &str| if t.is_empty() { 0.0 } else { setup.font_pt * 1.6 };
    (page, layout(img_w, img_h, page, m, reserve(&setup.header), reserve(&setup.footer), setup, 0.75))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_uses_landscape_page_for_wide_image() {
        let s = PageSetup::default();
        let (page, pages) = plan_preview(&s, 1600, 900);
        assert!(page.0 > page.1);
        assert_eq!(pages.len(), 1);
        let (page, _) = plan_preview(&s, 900, 1600);
        assert!(page.0 < page.1);
    }

    fn setup(fit: Fit) -> PageSetup {
        PageSetup { fit, center: false, margins: [1.0, 1.0, 1.0, 1.0], ..Default::default() }
    }

    #[test]
    fn fill_page_keeps_proportions_inside_margins() {
        // 8x10 inch page, 1 inch margins -> 6x8 area. A 600x200 image fills the width.
        let p = layout(600, 200, (8.0, 10.0), [1.0; 4], 0.0, 0.0, &setup(Fit::FillPage), 1.0);
        assert_eq!(p.len(), 1);
        assert!((p[0].w - 6.0).abs() < 1e-4 && (p[0].h - 2.0).abs() < 1e-4);
        assert!((p[0].x - 1.0).abs() < 1e-4 && (p[0].y - 1.0).abs() < 1e-4);
    }

    #[test]
    fn centering_places_image_in_the_middle() {
        let s = PageSetup { center: true, ..setup(Fit::Scale) };
        let p = layout(100, 100, (8.0, 10.0), [1.0; 4], 0.0, 0.0, &PageSetup { scale_percent: 100.0, ..s }, 0.01);
        // 1x1 inch image in a 6x8 area.
        assert!((p[0].x - 3.5).abs() < 1e-3);
        assert!((p[0].y - 4.5).abs() < 1e-3);
    }

    #[test]
    fn page_width_continues_over_several_pages() {
        // 6x8 area; a 100x300 image scaled to width 6 is 18 tall -> 133 rows fit a page -> 3 pages.
        let p = layout(100, 300, (8.0, 10.0), [1.0; 4], 0.0, 0.0, &setup(Fit::PageWidth), 1.0);
        assert_eq!(p.len(), 3);
        assert_eq!(p.iter().map(|x| x.src_h).sum::<u32>(), 300);
        assert_eq!(p[1].src_y, p[0].src_h);
        assert!(p.iter().all(|x| x.h <= 8.0 + 1e-3));
    }

    #[test]
    fn auto_fit_only_when_scale_is_too_large() {
        let s = PageSetup { scale_percent: 100.0, auto_fit_if_too_large: true, ..setup(Fit::Scale) };
        let big = layout(1000, 1000, (8.0, 10.0), [1.0; 4], 0.0, 0.0, &s, 0.01); // 10x10 inch
        assert!(big[0].w <= 6.0 + 1e-3);
        let small = layout(200, 200, (8.0, 10.0), [1.0; 4], 0.0, 0.0, &s, 0.01); // 2x2 inch
        assert!((small[0].w - 2.0).abs() < 1e-3);
    }

    #[test]
    fn header_space_is_reserved() {
        let p = layout(100, 100, (8.0, 10.0), [1.0; 4], 0.5, 0.5, &setup(Fit::FillPage), 1.0);
        assert!((p[0].y - 1.5).abs() < 1e-3);
        assert!(p[0].y + p[0].h <= 9.0 - 0.5 + 1e-3);
    }

    #[test]
    fn best_fit_picks_landscape_for_wide_images() {
        let s = PageSetup::default();
        assert!(landscape_for(&s, 200, 100));
        assert!(!landscape_for(&s, 100, 200));
        let manual = PageSetup { best_fit: false, landscape: true, ..PageSetup::default() };
        assert!(landscape_for(&manual, 100, 200));
    }

    #[test]
    fn macros_expand() {
        assert_eq!(expand_text("Page %page% of %pages% - %file%", 2, 5, "a.png"), "Page 2 of 5 - a.png");
    }
}
