//! Full-screen overlay used for region, window, free-hand, multi-region and pan
//! selection. Runs as its own process (see `Params::to_args`) so the main window
//! never ends up in the shot and there is only one event loop per process.

use crate::capture::{self, Rect};
use crate::compose::{self, Compose};
use eframe::egui::{self, Color32, Pos2, Stroke};
use image::RgbaImage;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Region,
    Window,
    FreeHand,
    Multi,
    /// Slide a box the size of the previous region around the screen.
    Pan,
    /// Pick the window to auto-scroll; yields a 1x1 image at the clicked point.
    ScrollTarget,
}

impl Mode {
    pub fn as_arg(self) -> &'static str {
        match self {
            Mode::Region => "region",
            Mode::Window => "window",
            Mode::FreeHand => "freehand",
            Mode::Multi => "multi",
            Mode::Pan => "pan",
            Mode::ScrollTarget => "scrolltarget",
        }
    }
    pub fn from_arg(s: &str) -> Mode {
        match s {
            "window" => Mode::Window,
            "freehand" => Mode::FreeHand,
            "multi" => Mode::Multi,
            "pan" => Mode::Pan,
            "scrolltarget" => Mode::ScrollTarget,
            _ => Mode::Region,
        }
    }
}

#[derive(Clone)]
pub struct Params {
    pub mode: Mode,
    pub out: PathBuf,
    pub include_cursor: bool,
    pub bg: [u8; 4],
    pub compose: Compose,
    pub margin: u32,
    pub pan_size: Option<(i32, i32)>,
    pub show_help: bool,
    pub multi_start_window: bool,
}

impl Params {
    pub fn to_args(&self) -> Vec<String> {
        let (pw, ph) = self.pan_size.unwrap_or((0, 0));
        vec![
            "--pick".into(),
            self.mode.as_arg().into(),
            self.out.to_string_lossy().into_owned(),
            (self.include_cursor as u8).to_string(),
            self.bg.iter().map(|b| format!("{b:02x}")).collect(),
            self.compose.as_arg().into(),
            self.margin.to_string(),
            format!("{pw}x{ph}"),
            (self.show_help as u8).to_string(),
            (self.multi_start_window as u8).to_string(),
        ]
    }

    /// Parses the arguments after `--pick`.
    pub fn from_args(a: &[String]) -> Option<Params> {
        let get = |i: usize| a.get(i).map(String::as_str);
        let hex = get(3)?;
        let mut bg = [255u8; 4];
        for (i, b) in bg.iter_mut().enumerate() {
            *b = hex.get(i * 2..i * 2 + 2).and_then(|h| u8::from_str_radix(h, 16).ok()).unwrap_or(255);
        }
        let pan = get(6).and_then(|s| s.split_once('x')).and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?))).filter(|(w, h): &(i32, i32)| *w > 0 && *h > 0);
        Some(Params {
            mode: Mode::from_arg(get(0)?),
            out: PathBuf::from(get(1)?),
            include_cursor: get(2) == Some("1"),
            bg,
            compose: Compose::from_arg(get(4).unwrap_or("")),
            margin: get(5).and_then(|m| m.parse().ok()).unwrap_or(0),
            pan_size: pan,
            show_help: get(7) != Some("0"),
            multi_start_window: get(8) == Some("1"),
        })
    }
}

enum Commit {
    Rect(Rect),
    Free,
    Multi,
}

struct Picker {
    p: Params,
    shot: RgbaImage,
    origin: (i32, i32),
    tex: Option<egui::TextureHandle>,
    windows: Vec<capture::WindowEntry>,
    drag_start: Option<(i32, i32)>,
    click_start: Option<(i32, i32)>,
    path: Vec<(i32, i32)>,
    parts: Vec<Rect>,
    multi_window: bool,
    done: bool,
}

pub fn run(p: Params) -> i32 {
    let vs = capture::virtual_screen();
    let windows = if matches!(p.mode, Mode::Window | Mode::Multi | Mode::ScrollTarget) { capture::visible_windows() } else { vec![] };
    let shot = match capture::capture_rect(vs, p.include_cursor) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("capture failed: {e}");
            return 2;
        }
    };
    let ppp = unsafe { windows::Win32::UI::HiDpi::GetDpiForSystem() } as f32 / 96.0;
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_decorations(false)
            .with_always_on_top()
            .with_taskbar(false)
            .with_position(Pos2::new(vs.x as f32 / ppp, vs.y as f32 / ppp))
            .with_inner_size([vs.w as f32 / ppp, vs.h as f32 / ppp]),
        ..Default::default()
    };
    let result = std::rc::Rc::new(std::cell::Cell::new(1));
    let r2 = result.clone();
    let multi_window = p.multi_start_window;
    let _ = eframe::run_native(
        "hsnap-picker",
        opts,
        Box::new(move |_| {
            Ok(Box::new(PickerApp {
                inner: Picker { p, shot, origin: (vs.x, vs.y), tex: None, windows, drag_start: None, click_start: None, path: vec![], parts: vec![], multi_window, done: false },
                result: r2,
            }))
        }),
    );
    result.get()
}

struct PickerApp {
    inner: Picker,
    result: std::rc::Rc<std::cell::Cell<i32>>,
}

impl Picker {
    fn finish(&mut self, img: RgbaImage, screen_rect: Option<Rect>) -> i32 {
        if let Some(r) = screen_rect {
            let _ = std::fs::write(self.p.out.with_extension("rect"), format!("{} {} {} {}", r.x, r.y, r.w, r.h));
        }
        if img.save(&self.p.out).is_ok() { 0 } else { 2 }
    }

    fn crop(&self, s: Rect) -> Option<(RgbaImage, Rect)> {
        let (iw, ih) = (self.shot.width() as i32, self.shot.height() as i32);
        let x = s.x.clamp(0, iw - 1);
        let y = s.y.clamp(0, ih - 1);
        let w = s.w.min(iw - x);
        let h = s.h.min(ih - y);
        if w <= 0 || h <= 0 {
            return None;
        }
        let img = image::imageops::crop_imm(&self.shot, x as u32, y as u32, w as u32, h as u32).to_image();
        Some((img, Rect { x: x + self.origin.0, y: y + self.origin.1, w, h }))
    }

    fn hint(&self) -> &'static str {
        match self.p.mode {
            Mode::Region => "Drag, or click twice, to select a region. Esc / right-click cancels.",
            Mode::Window => "Click a window or control to capture it. Esc / right-click cancels.",
            Mode::FreeHand => "Draw around the area. Enter, right-click, or clicking near the start finishes. Esc cancels.",
            Mode::Multi => "Drag regions (or click windows). Right-click switches region/window, Backspace removes the last, Enter finishes, Esc cancels.",
            Mode::Pan => "Move the box and click to capture a region of the previous size. Esc / right-click cancels.",
            Mode::ScrollTarget => "Click inside the window to capture with auto-scroll (not on a scroll bar). Esc cancels; Esc also stops scrolling.",
        }
    }
}

const RED: Color32 = Color32::from_rgb(255, 60, 60);
const BLUE: Color32 = Color32::from_rgb(70, 160, 255);

fn px_rect(s: Rect, ppp: f32) -> egui::Rect {
    egui::Rect::from_min_size(Pos2::new(s.x as f32 / ppp, s.y as f32 / ppp), egui::vec2(s.w as f32 / ppp, s.h as f32 / ppp))
}

fn dim_around(painter: &egui::Painter, full: egui::Rect, hole: Option<egui::Rect>, alpha: u8) {
    let dim = Color32::from_black_alpha(alpha);
    match hole {
        None => {
            painter.rect_filled(full, 0.0, dim);
        }
        Some(sel) => {
            painter.rect_filled(egui::Rect::from_min_max(full.min, Pos2::new(full.max.x, sel.min.y)), 0.0, dim);
            painter.rect_filled(egui::Rect::from_min_max(Pos2::new(full.min.x, sel.max.y), full.max), 0.0, dim);
            painter.rect_filled(egui::Rect::from_min_max(Pos2::new(full.min.x, sel.min.y), Pos2::new(sel.min.x, sel.max.y)), 0.0, dim);
            painter.rect_filled(egui::Rect::from_min_max(Pos2::new(sel.max.x, sel.min.y), Pos2::new(full.max.x, sel.max.y)), 0.0, dim);
        }
    }
}

/// Magnifier next to the pointer: 15x15 source pixels at 9x, with the centre pixel marked.
fn loupe(painter: &egui::Painter, tex: &egui::TextureHandle, hover: (i32, i32), img_size: (f32, f32), ppp: f32, full: egui::Rect, caption: &str) {
    const R: i32 = 7;
    const CELL: f32 = 9.0;
    let side = (2 * R + 1) as f32 * CELL;
    let cur = Pos2::new(hover.0 as f32 / ppp, hover.1 as f32 / ppp);
    let mut min = cur + egui::vec2(24.0, 24.0);
    if min.x + side > full.max.x {
        min.x = cur.x - 24.0 - side;
    }
    if min.y + side + 20.0 > full.max.y {
        min.y = cur.y - 24.0 - side - 20.0;
    }
    let r = egui::Rect::from_min_size(min, egui::vec2(side, side));
    let uv = egui::Rect::from_min_max(
        Pos2::new((hover.0 - R) as f32 / img_size.0, (hover.1 - R) as f32 / img_size.1),
        Pos2::new((hover.0 + R + 1) as f32 / img_size.0, (hover.1 + R + 1) as f32 / img_size.1),
    );
    painter.rect_filled(r.expand(2.0), 0.0, Color32::BLACK);
    painter.image(tex.id(), r, uv, Color32::WHITE);
    let c = egui::Rect::from_min_size(r.min + egui::vec2(R as f32 * CELL, R as f32 * CELL), egui::vec2(CELL, CELL));
    painter.rect_stroke(c, 0.0, Stroke::new(1.5, RED), egui::StrokeKind::Outside);
    painter.rect_filled(egui::Rect::from_min_size(Pos2::new(r.min.x, r.max.y + 2.0), egui::vec2(side, 18.0)), 0.0, Color32::BLACK);
    painter.text(Pos2::new(r.min.x + 4.0, r.max.y + 3.0), egui::Align2::LEFT_TOP, caption, egui::FontId::monospace(12.0), Color32::WHITE);
}

impl eframe::App for PickerApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let p = &mut self.inner;
        let ppp = ctx.pixels_per_point();
        let tex = p.tex.get_or_insert_with(|| {
            let ci = egui::ColorImage::from_rgba_unmultiplied([p.shot.width() as usize, p.shot.height() as usize], p.shot.as_raw());
            ctx.load_texture("shot", ci, egui::TextureOptions::NEAREST)
        });
        let tex = tex.clone();
        let (iw, ih) = (p.shot.width() as f32, p.shot.height() as f32);

        let (esc, enter, backspace, rclick) = ctx.input(|i| {
            (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Backspace), i.pointer.button_clicked(egui::PointerButton::Secondary))
        });
        if esc {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        // Right-click: cancel, except where it has a job of its own.
        let mut finish_free = false;
        if rclick {
            match p.p.mode {
                Mode::FreeHand => finish_free = true,
                Mode::Multi => p.multi_window = !p.multi_window,
                _ => {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    return;
                }
            }
        }

        let mut selection: Option<Rect> = None; // image pixels
        let mut commit: Option<Commit> = None;
        let mut hover_px: Option<(i32, i32)> = None;
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            let full = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(iw / ppp, ih / ppp));
            let painter = ui.painter().clone();
            painter.image(tex.id(), full, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let resp = ui.allocate_rect(full, egui::Sense::click_and_drag());
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            let to_px = |pt: Pos2| ((pt.x * ppp) as i32, (pt.y * ppp) as i32);
            let hover = ctx.input(|i| i.pointer.hover_pos()).map(to_px);
            hover_px = hover;
            // egui reports a drag only after the pointer has moved; the press point is the real start.
            let press = ctx.input(|i| i.pointer.press_origin()).map(to_px);
            let (ox, oy) = p.origin;

            // The innermost control under the pointer, so single buttons and panes can be picked.
            let window_at = |p: &Picker, hx: i32, hy: i32| capture::target_at(&p.windows, hx + ox, hy + oy, true).map(|w| Rect { x: w.x - ox, y: w.y - oy, w: w.w, h: w.h });

            match p.p.mode {
                Mode::Region | Mode::Multi if !(p.p.mode == Mode::Multi && p.multi_window) => {
                    if resp.drag_started() {
                        p.drag_start = press.or(hover);
                    }
                    if p.p.mode == Mode::Region && resp.clicked() && p.drag_start.is_none() {
                        match (p.click_start, hover) {
                            (None, Some(h)) => p.click_start = Some(h),
                            (Some(s), Some(h)) => {
                                let r = Rect::from_corners(s, h);
                                if r.w > 0 && r.h > 0 {
                                    commit = Some(Commit::Rect(r));
                                }
                            }
                            _ => {}
                        }
                    }
                    let start = p.drag_start.or(p.click_start);
                    if let (Some(s), Some(h)) = (start, hover) {
                        selection = Some(Rect::from_corners(s, h));
                    }
                    if resp.drag_stopped() {
                        if let Some(sel) = selection.take() {
                            if p.p.mode == Mode::Region {
                                commit = Some(Commit::Rect(sel));
                            } else if sel.w > 2 && sel.h > 2 {
                                p.parts.push(sel);
                            }
                        }
                        p.drag_start = None;
                    }
                }
                Mode::Window | Mode::Multi | Mode::ScrollTarget => {
                    if let Some((hx, hy)) = hover {
                        selection = window_at(p, hx, hy);
                        if resp.clicked() {
                            if let Some(sel) = selection {
                                if p.p.mode == Mode::Window {
                                    commit = Some(Commit::Rect(sel));
                                } else if p.p.mode == Mode::ScrollTarget {
                                    commit = Some(Commit::Rect(Rect { x: hx, y: hy, w: 1, h: 1 }));
                                } else {
                                    p.parts.push(sel);
                                }
                            }
                        }
                    }
                }
                Mode::Pan => {
                    if let (Some((hx, hy)), Some((w, h))) = (hover, p.p.pan_size) {
                        let r = Rect { x: (hx - w / 2).clamp(0, (iw as i32 - w).max(0)), y: (hy - h / 2).clamp(0, (ih as i32 - h).max(0)), w, h };
                        selection = Some(r);
                        if resp.clicked() {
                            commit = Some(Commit::Rect(r));
                        }
                    }
                }
                Mode::Region => {} // handled by the first arm (its guard only excludes Multi)
                Mode::FreeHand => {
                    if resp.drag_started() {
                        if let Some(s) = press {
                            p.path.push(s);
                        }
                    }
                    if resp.dragged() {
                        if let Some(h) = hover {
                            if p.path.last().is_none_or(|l| (l.0 - h.0).abs() + (l.1 - h.1).abs() > 1) {
                                p.path.push(h);
                            }
                        }
                    } else if resp.clicked() && p.path.len() > 3 {
                        if let (Some(h), Some(s)) = (hover, p.path.first()) {
                            if (h.0 - s.0).abs() < 10 && (h.1 - s.1).abs() < 10 {
                                finish_free = true;
                            }
                        }
                    }
                }
            }
            if p.p.mode == Mode::Multi && backspace {
                p.parts.pop();
            }
            if enter {
                match p.p.mode {
                    Mode::FreeHand => finish_free = true,
                    Mode::Multi if !p.parts.is_empty() => commit = Some(Commit::Multi),
                    _ => {}
                }
            }
            if finish_free && p.path.len() >= 3 {
                commit = Some(Commit::Free);
            }

            // Dimming, outlines, hints.
            let sel_rect = selection.map(|s| px_rect(s, ppp));
            let alpha = if p.p.mode == Mode::FreeHand { 60 } else { 110 };
            dim_around(&painter, full, sel_rect, alpha);
            for (i, part) in p.parts.iter().enumerate() {
                let r = px_rect(*part, ppp);
                painter.rect_stroke(r, 0.0, Stroke::new(2.0, BLUE), egui::StrokeKind::Outside);
                painter.text(r.min + egui::vec2(4.0, 2.0), egui::Align2::LEFT_TOP, (i + 1).to_string(), egui::FontId::proportional(14.0), Color32::YELLOW);
            }
            if let Some(r) = sel_rect {
                painter.rect_stroke(r, 0.0, Stroke::new(1.5, RED), egui::StrokeKind::Outside);
                let s = selection.unwrap();
                painter.text(r.min + egui::vec2(4.0, -16.0), egui::Align2::LEFT_TOP, format!("{} x {}", s.w, s.h), egui::FontId::proportional(13.0), Color32::YELLOW);
            }
            if p.p.mode == Mode::FreeHand && !p.path.is_empty() {
                let pts: Vec<Pos2> = p.path.iter().map(|q| Pos2::new(q.0 as f32 / ppp, q.1 as f32 / ppp)).collect();
                painter.add(egui::Shape::line(pts.clone(), Stroke::new(2.0, RED)));
                if let (Some(a), Some(b)) = (pts.first(), pts.last()) {
                    painter.line_segment([*a, *b], Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 60, 60, 120)));
                    painter.circle_stroke(*a, 5.0, Stroke::new(1.5, Color32::YELLOW));
                }
            }
            if p.p.show_help {
                let mut hint = p.hint().to_string();
                if p.p.mode == Mode::Multi {
                    hint += &format!("  [now: {}, {} selected]", if p.multi_window { "window" } else { "region" }, p.parts.len());
                }
                painter.rect_filled(egui::Rect::from_min_size(Pos2::new(16.0, 16.0), egui::vec2(hint.chars().count() as f32 * 7.6 + 16.0, 26.0)), 4.0, Color32::from_black_alpha(190));
                painter.text(Pos2::new(24.0, 21.0), egui::Align2::LEFT_TOP, hint, egui::FontId::proportional(14.0), Color32::WHITE);
                if !matches!(p.p.mode, Mode::Window | Mode::ScrollTarget) {
                    if let Some(h) = hover {
                        let cap = match selection {
                            Some(s) => format!("{},{}  {}x{}", h.0 + ox, h.1 + oy, s.w, s.h),
                            None => format!("{},{}", h.0 + ox, h.1 + oy),
                        };
                        loupe(&painter, &tex, h, (iw, ih), ppp, full, &cap);
                    }
                }
            }
        });
        let _ = hover_px;

        if let (Some(c), false) = (commit, p.done) {
            let outcome = match c {
                Commit::Rect(s) => p.crop(s).map(|(img, scr)| p.finish(img, Some(scr))),
                Commit::Free => compose::freehand_crop(&p.shot, &p.path, p.p.bg).map(|(img, _)| p.finish(img, None)),
                Commit::Multi => {
                    let parts: Vec<((i32, i32), RgbaImage)> = p.parts.iter().filter_map(|r| p.crop(*r).map(|(img, scr)| ((scr.x, scr.y), img))).collect();
                    compose::compose_regions(&parts, p.p.compose, p.p.margin, p.p.bg).map(|img| p.finish(img, None))
                }
            };
            if let Some(code) = outcome {
                p.done = true;
                self.result.set(code);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
