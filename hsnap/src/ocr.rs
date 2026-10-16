//! Text recognition with the Windows OCR engine (Windows.Media.Ocr).

use image::{RgbaImage, imageops};
use windows::Globalization::Language;
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine;
use windows::Storage::Streams::DataWriter;
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize};
use windows::core::HSTRING;

/// One recognised word with its box in the OCR image's pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Line {
    pub words: Vec<Word>,
}

fn engine(lang: &str) -> Result<OcrEngine, String> {
    unsafe {
        // Already-initialised is fine; the result only matters for the first call on a thread.
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    if !lang.is_empty() {
        let l = Language::CreateLanguage(&HSTRING::from(lang)).map_err(|e| e.to_string())?;
        if OcrEngine::IsLanguageSupported(&l).unwrap_or(false) {
            return OcrEngine::TryCreateFromLanguage(&l).map_err(|e| e.to_string());
        }
        return Err(format!("OCR language {lang} is not installed"));
    }
    OcrEngine::TryCreateFromUserProfileLanguages().map_err(|_| "No OCR language is installed (Settings > Time & Language > Language)".to_string())
}

/// Tags of the OCR languages installed on this machine.
pub fn available_languages() -> Vec<String> {
    unsafe {
        let _ = RoInitialize(RO_INIT_MULTITHREADED);
    }
    OcrEngine::AvailableRecognizerLanguages()
        .map(|v| v.into_iter().filter_map(|l| l.LanguageTag().ok().map(|t| t.to_string())).collect())
        .unwrap_or_default()
}

fn bitmap(img: &RgbaImage) -> Result<SoftwareBitmap, String> {
    let e = |e: windows::core::Error| e.to_string();
    let mut bgra = Vec::with_capacity(img.as_raw().len());
    for p in img.as_raw().chunks_exact(4) {
        bgra.extend_from_slice(&[p[2], p[1], p[0], 255]);
    }
    let writer = DataWriter::new().map_err(e)?;
    writer.WriteBytes(&bgra).map_err(e)?;
    let buf = writer.DetachBuffer().map_err(e)?;
    let sb = SoftwareBitmap::Create(BitmapPixelFormat::Bgra8, img.width() as i32, img.height() as i32).map_err(e)?;
    sb.CopyFromBuffer(&buf).map_err(e)?;
    Ok(sb)
}

/// Recognise `img` (one image, within the engine's size limit) and return its lines.
pub fn recognize(img: &RgbaImage, lang: &str) -> Result<Vec<Line>, String> {
    let eng = engine(lang)?;
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600);
    // Small text reads better enlarged; never exceed the engine limit.
    let up = if img.width().max(img.height()) * 2 <= max && img.height() < 400 { 2 } else { 1 };
    let work = if up == 2 { imageops::resize(img, img.width() * 2, img.height() * 2, imageops::FilterType::Lanczos3) } else { img.clone() };
    if work.width().max(work.height()) > max {
        return Err(format!("image is larger than the OCR limit of {max}px"));
    }
    let result = eng.RecognizeAsync(&bitmap(&work)?).map_err(|e| e.to_string())?.get().map_err(|e| e.to_string())?;
    let mut lines = vec![];
    for l in result.Lines().map_err(|e| e.to_string())? {
        let mut line = Line::default();
        for w in l.Words().map_err(|e| e.to_string())? {
            let r = w.BoundingRect().map_err(|e| e.to_string())?;
            let s = up as f32;
            line.words.push(Word { text: w.Text().map_err(|e| e.to_string())?.to_string(), x: r.X / s, y: r.Y / s, w: r.Width / s, h: r.Height / s });
        }
        lines.push(line);
    }
    Ok(lines)
}

/// OCR an image of any height by cutting it at blank rows into pieces the engine accepts.
pub fn recognize_tall(img: &RgbaImage, lang: &str) -> Result<Vec<Line>, String> {
    let max = OcrEngine::MaxImageDimension().unwrap_or(2600).min(4000);
    let mut out = vec![];
    for (top, h) in split_at_blank_rows(img, max) {
        let piece = imageops::crop_imm(img, 0, top, img.width(), h).to_image();
        for mut l in recognize(&piece, lang)? {
            for w in &mut l.words {
                w.y += top as f32;
            }
            out.push(l);
        }
    }
    Ok(out)
}

/// Row ranges (top, height) no taller than `max_h`, preferring to cut where a whole row is one colour.
pub fn split_at_blank_rows(img: &RgbaImage, max_h: u32) -> Vec<(u32, u32)> {
    let (w, h) = img.dimensions();
    let blank = |y: u32| {
        let first = img.get_pixel(0, y);
        (1..w).all(|x| img.get_pixel(x, y) == first)
    };
    let mut parts = vec![];
    let mut top = 0;
    while top < h {
        if h - top <= max_h {
            parts.push((top, h - top));
            break;
        }
        let limit = top + max_h;
        // Look back up to 20% of the piece for a blank row to cut at.
        let cut = (limit - max_h / 5..limit).rev().find(|&y| blank(y)).unwrap_or(limit);
        parts.push((top, cut - top));
        top = cut;
    }
    parts
}

fn is_cjk(c: char) -> bool {
    matches!(c as u32, 0x3000..=0x303F | 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xFF00..=0xFFEF | 0xAC00..=0xD7AF)
}

/// The engine puts spaces between CJK characters; remove those but keep spaces next to Latin text.
pub fn join_words(words: &[Word]) -> String {
    let mut s = String::new();
    for w in words {
        if let (Some(prev), Some(next)) = (s.chars().last(), w.text.chars().next()) {
            if !(is_cjk(prev) && is_cjk(next)) {
                s.push(' ');
            }
        }
        s += &w.text;
    }
    s
}

/// Plain text, one recognised line per output line.
pub fn plain_text(lines: &[Line]) -> String {
    lines.iter().map(|l| join_words(&l.words)).collect::<Vec<_>>().join("\n")
}

/// Merge lines that sit on the same row (the engine splits a table row at wide gaps).
pub fn merge_rows(lines: &[Line]) -> Vec<Line> {
    let mut rows: Vec<(f32, f32, Line)> = vec![]; // (centre y, height, words)
    for l in lines {
        for w in &l.words {
            let cy = w.y + w.h / 2.0;
            match rows.iter_mut().find(|(c, h, _)| (cy - *c).abs() < h.max(w.h) * 0.5) {
                Some((_, _, row)) => row.words.push(w.clone()),
                None => rows.push((cy, w.h, Line { words: vec![w.clone()] })),
            }
        }
    }
    rows.sort_by(|a, b| a.0.total_cmp(&b.0));
    rows.into_iter()
        .map(|(_, _, mut r)| {
            r.words.sort_by(|a, b| a.x.total_cmp(&b.x));
            r
        })
        .collect()
}

/// Tab-delimited text: a tab replaces any gap clearly wider than a word space.
pub fn tab_text(lines: &[Line]) -> String {
    merge_rows(lines)
        .iter()
        .map(|l| {
            let mut s = String::new();
            let mut prev: Option<&Word> = None;
            for w in &l.words {
                if let Some(p) = prev {
                    let gap = w.x - (p.x + p.w);
                    let glyph = (p.h * 0.6).max(1.0);
                    if gap > glyph * 2.5 {
                        s.push('\t');
                    } else if !(is_cjk(p.text.chars().last().unwrap_or(' ')) && is_cjk(w.text.chars().next().unwrap_or(' '))) {
                        s.push(' ');
                    }
                }
                s += &w.text;
                prev = Some(w);
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(t: &str, x: f32, wd: f32) -> Word {
        Word { text: t.into(), x, y: 0.0, w: wd, h: 20.0 }
    }

    #[test]
    fn cjk_words_are_joined_without_spaces() {
        let words = [w("你", 0.0, 10.0), w("好", 12.0, 10.0), w("world", 30.0, 40.0), w("ok", 80.0, 20.0)];
        assert_eq!(join_words(&words), "你好 world ok");
    }

    #[test]
    fn split_lines_on_one_row_are_merged() {
        let a = Line { words: vec![Word { text: "Name".into(), x: 0.0, y: 100.0, w: 40.0, h: 20.0 }] };
        let b = Line { words: vec![Word { text: "Size".into(), x: 500.0, y: 102.0, w: 30.0, h: 20.0 }] };
        let c = Line { words: vec![Word { text: "next".into(), x: 0.0, y: 160.0, w: 40.0, h: 20.0 }] };
        assert_eq!(tab_text(&[a, b, c]), "Name	Size
next");
    }

    #[test]
    fn wide_gaps_become_tabs() {
        let line = Line { words: vec![w("Name", 0.0, 40.0), w("Size", 45.0, 30.0), w("Date", 300.0, 40.0)] };
        assert_eq!(tab_text(&[line]), "Name Size\tDate");
    }

    #[test]
    fn splitting_prefers_blank_rows() {
        let mut img = RgbaImage::from_pixel(10, 100, image::Rgba([255, 255, 255, 255]));
        for y in 0..100 {
            if y != 45 {
                img.put_pixel(3, y, image::Rgba([0, 0, 0, 255])); // every row has ink except row 45
            }
        }
        let parts = split_at_blank_rows(&img, 50);
        assert_eq!(parts, vec![(0, 45), (45, 50), (95, 5)]); // first cut at the blank row, the rest at the size limit
        assert_eq!(split_at_blank_rows(&img, 200), vec![(0, 100)]);
    }

    #[test]
    fn pieces_cover_the_image_exactly() {
        let img = RgbaImage::from_fn(5, 333, |_, y| image::Rgba([(y % 7) as u8, 0, 0, 255]));
        let parts = split_at_blank_rows(&img, 100);
        assert_eq!(parts.iter().map(|p| p.1).sum::<u32>(), 333);
        assert!(parts.windows(2).all(|p| p[0].0 + p[0].1 == p[1].0));
        assert!(parts.iter().all(|p| p.1 <= 100));
    }
}
