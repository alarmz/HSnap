//! Stamps: reusable image + text overlays (watermarks, headers, footers).
//!
//! Stamps are stored in our own format: `<name>.stamp.json` next to an optional
//! `<name>.png` holding the stamp's picture. They are not compatible with the
//! original program's stamp files.

use crate::editor;
use crate::print::Justify;
use image::{Rgba, RgbaImage, imageops};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

pub const MAX_STAMPS: usize = 40;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Anchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl Anchor {
    pub const ALL: [Anchor; 9] = [Anchor::TopLeft, Anchor::Top, Anchor::TopRight, Anchor::Left, Anchor::Center, Anchor::Right, Anchor::BottomLeft, Anchor::Bottom, Anchor::BottomRight];
    pub fn label(self) -> &'static str {
        match self {
            Anchor::TopLeft => "Top left",
            Anchor::Top => "Top",
            Anchor::TopRight => "Top right",
            Anchor::Left => "Left",
            Anchor::Center => "Center",
            Anchor::Right => "Right",
            Anchor::BottomLeft => "Bottom left",
            Anchor::Bottom => "Bottom",
            Anchor::BottomRight => "Bottom right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Placement {
    AtSelection,
    ExtendTop,
    ExtendBottom,
    /// Offsets are positive towards the picture's inside, as in the manual.
    OnPicture { h: i32, v: i32, from: Anchor },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stamp {
    pub name: String,
    pub use_image: bool,
    pub has_image: bool,
    pub use_text: bool,
    pub text: String,
    /// Text height in pixels.
    pub font_px: f32,
    pub text_color: [u8; 4],
    pub align: Justify,
    pub bg: [u8; 4],
    pub transparent_bg: bool,
    pub frame_width: u32,
    pub frame_color: [u8; 4],
    pub img_left: i32,
    pub img_top: i32,
    /// Text margins: left, top, right, bottom.
    pub margins: [i32; 4],
    pub width: u32,
    pub height: u32,
    pub placement: Placement,
    pub mask_color: Option<[u8; 4]>,
    /// 0 = opaque, 100 = fully blended away.
    pub blend: u8,
    pub auto_add: bool,
}

impl Default for Stamp {
    fn default() -> Self {
        Self {
            name: "New stamp".into(),
            use_image: true,
            has_image: false,
            use_text: true,
            text: "%date% %time%".into(),
            font_px: 18.0,
            text_color: [0, 0, 0, 255],
            align: Justify::Left,
            bg: [255, 255, 255, 255],
            transparent_bg: false,
            frame_width: 0,
            frame_color: [0, 0, 0, 255],
            img_left: 0,
            img_top: 0,
            margins: [6, 4, 6, 4],
            width: 260,
            height: 40,
            placement: Placement::OnPicture { h: 10, v: 10, from: Anchor::BottomRight },
            mask_color: None,
            blend: 0,
            auto_add: false,
        }
    }
}

/// Values the text macros expand to.
pub struct MacroCtx {
    pub file: String,
    pub path: String,
    pub user: String,
    pub date: String,
    pub time: String,
}

impl MacroCtx {
    pub fn now(file: &str, path: &str) -> Self {
        use windows::Win32::System::SystemInformation::GetLocalTime;
        let t = unsafe { GetLocalTime() };
        Self {
            file: file.into(),
            path: path.into(),
            user: std::env::var("USERNAME").unwrap_or_default(),
            date: format!("{:04}-{:02}-{:02}", t.wYear, t.wMonth, t.wDay),
            time: format!("{:02}:{:02}:{:02}", t.wHour, t.wMinute, t.wSecond),
        }
    }
}

pub fn expand_macros(text: &str, m: &MacroCtx) -> String {
    text.replace("%date%", &m.date).replace("%time%", &m.time).replace("%file%", &m.file).replace("%path%", &m.path).replace("%uname%", &m.user)
}

fn rgba(c: [u8; 4]) -> Rgba<u8> {
    Rgba(c)
}

/// Render a stamp. `extend_width` forces the width (header/footer placement), in which
/// case the height is derived from the content.
pub fn render(stamp: &Stamp, picture: Option<&RgbaImage>, m: &MacroCtx, extend_width: Option<u32>) -> RgbaImage {
    let pic = picture.filter(|_| stamp.use_image);
    let text = if stamp.use_text { expand_macros(&stamp.text, m) } else { String::new() };
    let fw = stamp.frame_width as i32;

    let width = extend_width.unwrap_or_else(|| {
        if stamp.width > 0 {
            stamp.width
        } else {
            let tw = text.lines().map(|l| editor::text_width(stamp.font_px, l)).fold(0.0f32, f32::max).ceil() as i32;
            let pw = pic.map_or(0, |p| stamp.img_left + p.width() as i32);
            (tw + stamp.margins[0] + stamp.margins[2]).max(pw).max(1) as u32
        }
    });
    let text_w = (width as i32 - stamp.margins[0] - stamp.margins[2] - 2 * fw).max(1) as f32;
    let lines = if text.is_empty() { vec![] } else { editor::wrap_text(stamp.font_px, &text, text_w) };
    let lh = editor::line_height(stamp.font_px);
    let text_h = (lines.len() as f32 * lh).ceil() as i32;

    let height = if extend_width.is_some() || stamp.height == 0 {
        let by_text = if lines.is_empty() { 0 } else { stamp.margins[1] + text_h + stamp.margins[3] };
        let by_pic = pic.map_or(0, |p| stamp.img_top + p.height() as i32);
        (by_text.max(by_pic) + 2 * fw).max(1) as u32
    } else {
        stamp.height
    };

    let mut out = RgbaImage::from_pixel(width, height, rgba(stamp.bg));
    if let Some(p) = pic {
        imageops::overlay(&mut out, p, stamp.img_left as i64 + fw as i64, stamp.img_top as i64 + fw as i64);
    }
    // Text, aligned inside the margins.
    let left = (stamp.margins[0] + fw) as f32;
    let top = (stamp.margins[1] + fw) as f32;
    for (i, line) in lines.iter().enumerate() {
        let w = editor::text_width(stamp.font_px, line);
        let x = match stamp.align {
            Justify::Left => left,
            Justify::Center => left + (text_w - w) / 2.0,
            Justify::Right => left + text_w - w,
        };
        editor::draw_text(&mut out, (x, top + i as f32 * lh), line, stamp.font_px, rgba(stamp.text_color));
    }
    // Frame.
    for i in 0..fw.max(0) as u32 {
        let (w, h) = (width.saturating_sub(2 * i), height.saturating_sub(2 * i));
        if w == 0 || h == 0 {
            break;
        }
        for x in 0..w {
            out.put_pixel(i + x, i, rgba(stamp.frame_color));
            out.put_pixel(i + x, i + h - 1, rgba(stamp.frame_color));
        }
        for y in 0..h {
            out.put_pixel(i, i + y, rgba(stamp.frame_color));
            out.put_pixel(i + w - 1, i + y, rgba(stamp.frame_color));
        }
    }
    // Transparency: background colour, mask colour, then the blend slider.
    let bg = rgba(stamp.bg);
    let mask = stamp.mask_color.map(rgba);
    let keep = (100 - stamp.blend.min(100)) as f32 / 100.0;
    for px in out.pixels_mut() {
        if (stamp.transparent_bg && *px == bg) || mask == Some(*px) {
            px[3] = 0;
        } else {
            px[3] = (px[3] as f32 * keep) as u8;
        }
    }
    out
}

/// Pixel position of an `OnPicture` stamp inside a `base_w` x `base_h` image.
pub fn anchor_position(base: (u32, u32), size: (u32, u32), h: i32, v: i32, from: Anchor) -> (i64, i64) {
    let (bw, bh, w, hh) = (base.0 as i64, base.1 as i64, size.0 as i64, size.1 as i64);
    let (col, row) = match from {
        Anchor::TopLeft => (0, 0),
        Anchor::Top => (1, 0),
        Anchor::TopRight => (2, 0),
        Anchor::Left => (0, 1),
        Anchor::Center => (1, 1),
        Anchor::Right => (2, 1),
        Anchor::BottomLeft => (0, 2),
        Anchor::Bottom => (1, 2),
        Anchor::BottomRight => (2, 2),
    };
    let x = match col {
        0 => h as i64,
        1 => (bw - w) / 2 + h as i64,
        _ => bw - w - h as i64, // right edge: positive offsets move left
    };
    let y = match row {
        0 => v as i64,
        1 => (bh - hh) / 2 + v as i64,
        _ => bh - hh - v as i64, // bottom edge: positive offsets move up
    };
    (x, y)
}

/// Place a stamp on an image. `selection` is (x, y, w, h) in image pixels.
pub fn apply(base: &RgbaImage, stamp: &Stamp, picture: Option<&RgbaImage>, m: &MacroCtx, selection: Option<(u32, u32, u32, u32)>) -> RgbaImage {
    match stamp.placement {
        Placement::ExtendTop | Placement::ExtendBottom => {
            let s = render(stamp, picture, m, Some(base.width()));
            let mut out = RgbaImage::from_pixel(base.width(), base.height() + s.height(), rgba(stamp.bg));
            let top = stamp.placement == Placement::ExtendTop;
            imageops::replace(&mut out, base, 0, if top { s.height() as i64 } else { 0 });
            imageops::overlay(&mut out, &s, 0, if top { 0 } else { base.height() as i64 });
            out
        }
        Placement::AtSelection => {
            let s = render(stamp, picture, m, None);
            let (x, y) = selection.map_or((0, 0), |(x, y, _, _)| (x as i64, y as i64));
            let mut out = base.clone();
            imageops::overlay(&mut out, &s, x, y);
            out
        }
        Placement::OnPicture { h, v, from } => {
            let s = render(stamp, picture, m, None);
            let (x, y) = anchor_position(base.dimensions(), s.dimensions(), h, v, from);
            let mut out = base.clone();
            imageops::overlay(&mut out, &s, x, y);
            out
        }
    }
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

pub struct Store {
    pub dir: PathBuf,
}

fn safe_name(name: &str) -> String {
    let s: String = name.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.') { c } else { '_' }).collect();
    let s = s.trim().trim_matches('.').to_string();
    if s.is_empty() { "stamp".into() } else { s }
}

impl Store {
    pub fn default_dir() -> PathBuf {
        std::env::var("APPDATA").map(|a| PathBuf::from(a).join("hsnap").join("stamps")).unwrap_or_else(|_| PathBuf::from("stamps"))
    }

    fn json_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.stamp.json", safe_name(name)))
    }
    fn png_path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.stamp.png", safe_name(name)))
    }

    /// All stamps, sorted by name. Unreadable files are skipped.
    pub fn list(&self) -> Vec<Stamp> {
        let mut v: Vec<Stamp> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".stamp.json"))
            .filter_map(|e| serde_json::from_str(&std::fs::read_to_string(e.path()).ok()?).ok())
            .collect();
        v.sort_by_key(|s: &Stamp| s.name.to_lowercase());
        v
    }

    pub fn picture(&self, stamp: &Stamp) -> Option<RgbaImage> {
        if !stamp.has_image {
            return None;
        }
        image::open(self.png_path(&stamp.name)).ok().map(|i| i.to_rgba8())
    }

    pub fn save(&self, stamp: &Stamp, picture: Option<&RgbaImage>) -> Result<(), String> {
        if self.list().iter().filter(|s| s.name != stamp.name).count() >= MAX_STAMPS {
            return Err(format!("A stamps folder holds at most {MAX_STAMPS} stamps; use another folder."));
        }
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        let mut s = stamp.clone();
        s.has_image = picture.is_some();
        match picture {
            Some(p) => p.save(self.png_path(&s.name)).map_err(|e| e.to_string())?,
            None => {
                let _ = std::fs::remove_file(self.png_path(&s.name));
            }
        }
        std::fs::write(self.json_path(&s.name), serde_json::to_string_pretty(&s).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
    }

    /// Delete a stamp; files go to the Recycle Bin.
    pub fn delete(&self, name: &str) -> Result<(), String> {
        let mut files = vec![self.json_path(name)];
        if self.png_path(name).exists() {
            files.push(self.png_path(name));
        }
        trash::delete_all(files).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> MacroCtx {
        MacroCtx { file: "a.png".into(), path: r"C:\x\a.png".into(), user: "me".into(), date: "2026-01-02".into(), time: "03:04:05".into() }
    }

    fn base() -> RgbaImage {
        RgbaImage::from_pixel(200, 100, Rgba([10, 20, 30, 255]))
    }

    #[test]
    fn macros_expand() {
        assert_eq!(expand_macros("%uname% %file% %date% %time% %path%", &ctx()), r"me a.png 2026-01-02 03:04:05 C:\x\a.png");
    }

    #[test]
    fn anchors_follow_the_manual_sign_rules() {
        // Right/bottom edges: positive offsets go inward (left/up).
        assert_eq!(anchor_position((200, 100), (40, 20), 10, 5, Anchor::BottomRight), (150, 75));
        assert_eq!(anchor_position((200, 100), (40, 20), 10, 5, Anchor::TopLeft), (10, 5));
        // Centre: 10 right and 20 down of the exact centre.
        assert_eq!(anchor_position((200, 100), (40, 20), 10, 20, Anchor::Center), (90, 60));
    }

    #[test]
    fn on_picture_overlays_in_place() {
        let st = Stamp { use_text: false, width: 20, height: 10, bg: [255, 0, 0, 255], placement: Placement::OnPicture { h: 0, v: 0, from: Anchor::TopLeft }, ..Default::default() };
        let out = apply(&base(), &st, None, &ctx(), None);
        assert_eq!(out.dimensions(), (200, 100));
        assert_eq!(out.get_pixel(5, 5), &Rgba([255, 0, 0, 255]));
        assert_eq!(out.get_pixel(50, 50), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn extend_adds_a_band_the_width_of_the_image() {
        let st = Stamp { text: "Footer text".into(), placement: Placement::ExtendBottom, bg: [255, 255, 255, 255], ..Default::default() };
        let out = apply(&base(), &st, None, &ctx(), None);
        assert_eq!(out.width(), 200);
        assert!(out.height() > 100);
        assert_eq!(out.get_pixel(0, 0), &Rgba([10, 20, 30, 255])); // original stays on top
        let top = apply(&base(), &Stamp { placement: Placement::ExtendTop, ..st }, None, &ctx(), None);
        assert_eq!(top.get_pixel(0, top.height() - 1), &Rgba([10, 20, 30, 255]));
    }

    #[test]
    fn transparent_background_and_blend() {
        let st = Stamp { use_text: false, width: 4, height: 4, bg: [1, 2, 3, 255], transparent_bg: true, ..Default::default() };
        assert_eq!(render(&st, None, &ctx(), None).get_pixel(1, 1)[3], 0);
        let half = Stamp { transparent_bg: false, blend: 50, ..st };
        assert_eq!(render(&half, None, &ctx(), None).get_pixel(1, 1)[3], 127);
    }

    #[test]
    fn mask_colour_makes_pixels_transparent() {
        let pic = RgbaImage::from_pixel(4, 4, Rgba([0, 255, 0, 255]));
        let st = Stamp { use_text: false, width: 4, height: 4, mask_color: Some([0, 255, 0, 255]), ..Default::default() };
        assert_eq!(render(&st, Some(&pic), &ctx(), None).get_pixel(2, 2)[3], 0);
    }

    #[test]
    fn frame_is_drawn_inside_the_edge() {
        let st = Stamp { use_text: false, width: 10, height: 10, frame_width: 2, frame_color: [9, 9, 9, 255], ..Default::default() };
        let out = render(&st, None, &ctx(), None);
        assert_eq!(out.get_pixel(0, 5), &Rgba([9, 9, 9, 255]));
        assert_eq!(out.get_pixel(1, 5), &Rgba([9, 9, 9, 255]));
        assert_eq!(out.get_pixel(2, 5), &Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn long_text_wraps_and_grows_the_extended_height() {
        let long = "word ".repeat(60);
        let st = Stamp { text: long, placement: Placement::ExtendTop, ..Default::default() };
        let tall = render(&st, None, &ctx(), Some(200)).height();
        let short = render(&Stamp { text: "word".into(), ..st }, None, &ctx(), Some(200)).height();
        assert!(tall > short * 3);
    }

    #[test]
    fn store_round_trip_and_limit() {
        let dir = std::env::temp_dir().join(format!("hsnap-stamps-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store { dir: dir.clone() };
        let pic = RgbaImage::from_pixel(3, 3, Rgba([1, 2, 3, 255]));
        store.save(&Stamp { name: "Logo: v1".into(), auto_add: true, ..Default::default() }, Some(&pic)).unwrap();
        store.save(&Stamp { name: "alpha".into(), ..Default::default() }, None).unwrap();
        let list = store.list();
        assert_eq!(list.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["alpha", "Logo: v1"]);
        let logo = &list[1];
        assert!(logo.auto_add && logo.has_image);
        assert_eq!(store.picture(logo).unwrap().dimensions(), (3, 3));
        assert!(store.picture(&list[0]).is_none());
        for i in 0..MAX_STAMPS {
            let _ = store.save(&Stamp { name: format!("s{i}"), ..Default::default() }, None);
        }
        assert!(store.list().len() <= MAX_STAMPS);
        assert!(store.save(&Stamp { name: "one too many".into(), ..Default::default() }, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
