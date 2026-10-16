//! Full-screen overlay used for region and window selection. Runs as its own
//! process (`hsnap --pick <region|window> <out.png> <include_cursor 0|1>`) so
//! the main window never ends up in the shot and there is only one event loop
//! per process.

use crate::capture::{self, Rect};
use eframe::egui::{self, Color32, Pos2, Stroke};
use image::RgbaImage;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Region,
    Window,
}

struct Picker {
    mode: Mode,
    shot: RgbaImage,
    origin: (i32, i32),
    tex: Option<egui::TextureHandle>,
    windows: Vec<Rect>,
    drag_start: Option<(i32, i32)>,
    out: PathBuf,
    done: bool,
}

pub fn run(mode: Mode, out: PathBuf, include_cursor: bool) -> i32 {
    let vs = capture::virtual_screen();
    let windows = if mode == Mode::Window { capture::visible_windows() } else { vec![] };
    let shot = match capture::capture_rect(vs, include_cursor) {
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
    let _ = eframe::run_native(
        "hsnap-picker",
        opts,
        Box::new(move |_| {
            Ok(Box::new(PickerApp { inner: Picker { mode, shot, origin: (vs.x, vs.y), tex: None, windows, drag_start: None, out, done: false }, result: r2 }))
        }),
    );
    result.get()
}

struct PickerApp {
    inner: Picker,
    result: std::rc::Rc<std::cell::Cell<i32>>,
}

impl eframe::App for PickerApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let p = &mut self.inner;
        let ppp = ctx.pixels_per_point();
        let tex = p.tex.get_or_insert_with(|| {
            let ci = egui::ColorImage::from_rgba_unmultiplied([p.shot.width() as usize, p.shot.height() as usize], p.shot.as_raw());
            ctx.load_texture("shot", ci, egui::TextureOptions::NEAREST)
        });
        let (iw, ih) = (p.shot.width() as f32, p.shot.height() as f32);

        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let rclick = ctx.input(|i| i.pointer.button_clicked(egui::PointerButton::Secondary));
        if esc || rclick {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        let mut selection: Option<Rect> = None; // image-pixel coordinates
        let mut commit = false;
        egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
            let full = egui::Rect::from_min_size(Pos2::ZERO, egui::vec2(iw / ppp, ih / ppp));
            let painter = ui.painter().clone();
            painter.image(tex.id(), full, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let resp = ui.allocate_rect(full, egui::Sense::click_and_drag());
            ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            let to_px = |pt: Pos2| ((pt.x * ppp) as i32, (pt.y * ppp) as i32);
            let hover = ctx.input(|i| i.pointer.hover_pos()).map(to_px);

            match p.mode {
                Mode::Region => {
                    if resp.drag_started() {
                        p.drag_start = hover;
                    }
                    if let (Some(s), Some(h)) = (p.drag_start, hover) {
                        selection = Some(Rect::from_corners(s, h));
                        if resp.drag_stopped() {
                            commit = true;
                        }
                    }
                }
                Mode::Window => {
                    if let Some((hx, hy)) = hover {
                        let (ox, oy) = p.origin;
                        selection = p
                            .windows
                            .iter()
                            .find(|w| w.contains(hx + ox, hy + oy))
                            .map(|w| Rect { x: w.x - ox, y: w.y - oy, w: w.w, h: w.h });
                        commit = resp.clicked() && selection.is_some();
                    }
                }
            }

            // Dim everything but the selection, outline it, show its size.
            let dim = Color32::from_black_alpha(110);
            match selection {
                Some(s) => {
                    let sel = egui::Rect::from_min_size(Pos2::new(s.x as f32 / ppp, s.y as f32 / ppp), egui::vec2(s.w as f32 / ppp, s.h as f32 / ppp));
                    painter.rect_filled(egui::Rect::from_min_max(full.min, Pos2::new(full.max.x, sel.min.y)), 0.0, dim);
                    painter.rect_filled(egui::Rect::from_min_max(Pos2::new(full.min.x, sel.max.y), full.max), 0.0, dim);
                    painter.rect_filled(egui::Rect::from_min_max(Pos2::new(full.min.x, sel.min.y), Pos2::new(sel.min.x, sel.max.y)), 0.0, dim);
                    painter.rect_filled(egui::Rect::from_min_max(Pos2::new(sel.max.x, sel.min.y), Pos2::new(full.max.x, sel.max.y)), 0.0, dim);
                    painter.rect_stroke(sel, 0.0, Stroke::new(1.5, Color32::from_rgb(255, 60, 60)), egui::StrokeKind::Outside);
                    painter.text(sel.min + egui::vec2(4.0, -16.0), egui::Align2::LEFT_TOP, format!("{} x {}", s.w, s.h), egui::FontId::proportional(13.0), Color32::YELLOW);
                }
                None => {
                    painter.rect_filled(full, 0.0, dim);
                    let msg = match p.mode {
                        Mode::Region => "Drag to select a region. Esc or right-click cancels.",
                        Mode::Window => "Click a window to capture it. Esc or right-click cancels.",
                    };
                    painter.text(Pos2::new(24.0, 24.0), egui::Align2::LEFT_TOP, msg, egui::FontId::proportional(16.0), Color32::WHITE);
                }
            }
        });

        if commit && !p.done {
            if let Some(s) = selection.filter(|s| s.w > 0 && s.h > 0) {
                p.done = true;
                let x = s.x.clamp(0, p.shot.width() as i32 - 1) as u32;
                let y = s.y.clamp(0, p.shot.height() as i32 - 1) as u32;
                let w = (s.w as u32).min(p.shot.width() - x);
                let h = (s.h as u32).min(p.shot.height() - y);
                let crop = image::imageops::crop_imm(&p.shot, x, y, w, h).to_image();
                // Screen rectangle next to the image so "repeat last capture" can reuse it.
                let rect_txt = format!("{} {} {} {}", x as i32 + p.origin.0, y as i32 + p.origin.1, w, h);
                let _ = std::fs::write(p.out.with_extension("rect"), rect_txt);
                self.result.set(if crop.save(&p.out).is_ok() { 0 } else { 2 });
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }
}
