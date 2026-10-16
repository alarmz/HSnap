//! Main window: capture toolbar, drawing tools, tabbed documents.

use crate::editor::{Document, Kind, Pt, Shape};
use crate::hotkeys::HotkeyAction;
use crate::quicksave::QuickSave;
use crate::service::{CaptureMode, Captured, Service, Settings, WINDOW_TITLE};
use eframe::egui::{self, Color32, Pos2, Sense, Stroke};
use std::sync::mpsc::Receiver;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Pencil,
    Line,
    Arrow,
    Rect,
    Ellipse,
    Text,
    Crop,
}

const TOOLS: &[(Tool, &str)] = &[
    (Tool::Pencil, "Pencil"),
    (Tool::Line, "Line"),
    (Tool::Arrow, "Arrow"),
    (Tool::Rect, "Rect"),
    (Tool::Ellipse, "Ellipse"),
    (Tool::Text, "Text"),
    (Tool::Crop, "Crop"),
];

pub struct App {
    svc: Service,
    captures: Receiver<Captured>,
    hotkeys: Receiver<HotkeyAction>,
    docs: Vec<Document>,
    current: usize,
    tex: Option<egui::TextureHandle>,
    tex_dirty: bool,
    tool: Tool,
    color: Color32,
    width: f32,
    drag: Option<Vec<Pt>>,
    pending_text: Option<(Pt, String)>,
    quicksave: Option<QuickSave>,
    counter: u32,
    status: String,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let svc = Service::new(tx, cc.egui_ctx.clone(), Settings::load());
        let ctx = cc.egui_ctx.clone();
        let hotkeys = crate::hotkeys::start(move || ctx.request_repaint());
        Self {
            svc,
            captures: rx,
            hotkeys,
            docs: vec![],
            current: 0,
            tex: None,
            tex_dirty: true,
            tool: Tool::Rect,
            color: Color32::from_rgb(230, 30, 30),
            width: 3.0,
            drag: None,
            pending_text: None,
            quicksave: None,
            counter: 0,
            status: "Ready. Ctrl+Shift+R region, Ctrl+Shift+S full screen, Ctrl+Shift+W window.".into(),
        }
    }

    fn settings(&self) -> Settings {
        self.svc.settings.lock().unwrap().clone()
    }

    fn on_capture(&mut self, c: Captured) {
        let s = self.settings();
        if s.auto_copy {
            copy_to_clipboard(&c.img);
        }
        let mut name = None;
        if s.quicksave {
            let qs = self.quicksave.get_or_insert_with(|| {
                QuickSave::new(&s.quicksave_template, s.quicksave_start, (!s.quicksave_stop.is_empty()).then_some(s.quicksave_stop.as_str()), s.quicksave_step, s.quicksave_loop)
            });
            match qs.next_name() {
                Some(n) => {
                    let path = std::path::Path::new(&s.quicksave_dir).join(&n);
                    match c.img.save(&path) {
                        Ok(()) => self.status = format!("Saved {}", path.display()),
                        Err(e) => self.status = format!("Save failed: {e}"),
                    }
                    name = Some(n);
                }
                None => self.status = "QuickSave reached its stop number.".into(),
            }
        } else {
            self.status = format!("{} captured ({}x{})", c.label, c.img.width(), c.img.height());
        }
        self.counter += 1;
        let name = name.unwrap_or_else(|| format!("Capture {}", self.counter));
        self.docs.push(Document::new(name, c.img));
        self.current = self.docs.len() - 1;
        self.tex_dirty = true;
    }

    fn doc(&mut self) -> Option<&mut Document> {
        self.docs.get_mut(self.current)
    }
}

fn copy_to_clipboard(img: &image::RgbaImage) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_image(arboard::ImageData {
            width: img.width() as usize,
            height: img.height() as usize,
            bytes: std::borrow::Cow::Borrowed(img.as_raw()),
        });
    }
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        while let Ok(a) = self.hotkeys.try_recv() {
            self.svc.handle_hotkey(a);
        }
        while let Ok(c) = self.captures.try_recv() {
            self.on_capture(c);
        }
        let _ = WINDOW_TITLE;

        egui::TopBottomPanel::top("capture").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (label, mode) in [
                    ("Full screen", CaptureMode::Full),
                    ("Virtual desktop", CaptureMode::Virtual),
                    ("Active window", CaptureMode::Active),
                    ("Window", CaptureMode::Window),
                    ("Region", CaptureMode::Region),
                    ("Cursor", CaptureMode::Cursor),
                ] {
                    if ui.button(label).clicked() {
                        self.svc.capture(mode);
                    }
                }
                if ui.button("Repeat last").clicked() {
                    self.svc.repeat_last();
                }
            });
            let mut s = self.settings();
            let before = (s.delay_ms, s.include_cursor, s.hide_window, s.auto_copy, s.quicksave, s.hotkeys_enabled);
            ui.horizontal_wrapped(|ui| {
                ui.label("Delay (ms)");
                ui.add(egui::DragValue::new(&mut s.delay_ms).range(0..=60_000).speed(50));
                ui.checkbox(&mut s.include_cursor, "Include cursor");
                ui.checkbox(&mut s.hide_window, "Hide this window");
                ui.checkbox(&mut s.auto_copy, "Auto-copy");
                ui.checkbox(&mut s.hotkeys_enabled, "Hot keys");
                ui.checkbox(&mut s.quicksave, "QuickSave");
                if s.quicksave {
                    ui.text_edit_singleline(&mut s.quicksave_dir);
                    ui.add(egui::TextEdit::singleline(&mut s.quicksave_template).desired_width(120.0));
                }
                ui.label("Repeat every (s)");
                ui.add(egui::DragValue::new(&mut s.repeat_secs).range(0.1..=3600.0).speed(0.5));
                if self.svc.is_auto_running() {
                    if ui.button("Stop (Shift+F11)").clicked() {
                        self.svc.stop_auto();
                    }
                } else if ui.button("Start auto").clicked() {
                    self.svc.start_auto(s.repeat_secs);
                }
            });
            let after = (s.delay_ms, s.include_cursor, s.hide_window, s.auto_copy, s.quicksave, s.hotkeys_enabled);
            let naming_changed = s.quicksave_template != self.settings().quicksave_template;
            if before != after || naming_changed {
                if naming_changed || before.4 != after.4 {
                    self.quicksave = None; // restart numbering with the new template
                }
                s.save();
                *self.svc.settings.lock().unwrap() = s;
            } else {
                *self.svc.settings.lock().unwrap() = s; // keep text-field edits
            }
        });

        egui::TopBottomPanel::top("tools").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                for (t, name) in TOOLS {
                    ui.selectable_value(&mut self.tool, *t, *name);
                }
                ui.separator();
                ui.color_edit_button_srgba(&mut self.color);
                ui.add(egui::Slider::new(&mut self.width, 1.0..=20.0).text("Width"));
                ui.separator();
                let (can_undo, can_redo) = self.docs.get(self.current).map_or((false, false), |d| (d.can_undo(), d.can_redo()));
                if ui.add_enabled(can_undo, egui::Button::new("Undo")).clicked() {
                    self.doc().unwrap().undo();
                    self.tex_dirty = true;
                }
                if ui.add_enabled(can_redo, egui::Button::new("Redo")).clicked() {
                    self.doc().unwrap().redo();
                    self.tex_dirty = true;
                }
                let has = !self.docs.is_empty();
                if ui.add_enabled(has, egui::Button::new("Rotate 90°")).clicked() {
                    self.doc().unwrap().rotate90();
                    self.tex_dirty = true;
                }
                if ui.add_enabled(has, egui::Button::new("Mirror")).clicked() {
                    self.doc().unwrap().flip_horizontal();
                    self.tex_dirty = true;
                }
                if ui.add_enabled(has, egui::Button::new("Copy")).clicked() {
                    let flat = self.docs[self.current].flatten();
                    copy_to_clipboard(&flat);
                    self.status = "Copied to clipboard".into();
                }
                if ui.add_enabled(has, egui::Button::new("Save as…")).clicked() {
                    let name = self.docs[self.current].name.clone();
                    if let Some(path) = rfd::FileDialog::new()
                        .set_file_name(format!("{name}.png"))
                        .add_filter("PNG", &["png"])
                        .add_filter("JPEG", &["jpg", "jpeg"])
                        .add_filter("BMP", &["bmp"])
                        .add_filter("GIF", &["gif"])
                        .save_file()
                    {
                        let flat = self.docs[self.current].flatten();
                        let res = if matches!(path.extension().and_then(|e| e.to_str()), Some("jpg" | "jpeg")) {
                            image::DynamicImage::ImageRgba8(flat).to_rgb8().save(&path) // JPEG has no alpha
                        } else {
                            flat.save(&path)
                        };
                        self.status = match res {
                            Ok(()) => {
                                self.docs[self.current].modified = false;
                                format!("Saved {}", path.display())
                            }
                            Err(e) => format!("Save failed: {e}"),
                        };
                    }
                }
            });
        });

        egui::TopBottomPanel::bottom("tabs").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                let mut close = None;
                for (i, d) in self.docs.iter().enumerate() {
                    let label = if d.modified { format!("{} *", d.name) } else { d.name.clone() };
                    if ui.selectable_label(i == self.current, label).clicked() {
                        self.current = i;
                        self.tex_dirty = true;
                    }
                    if ui.small_button("x").clicked() {
                        close = Some(i);
                    }
                    ui.separator();
                }
                if let Some(i) = close {
                    self.docs.remove(i);
                    self.current = self.current.min(self.docs.len().saturating_sub(1));
                    self.tex_dirty = true;
                }
                ui.label(&self.status);
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| self.canvas(ui, ctx));
    }
}

impl App {
    fn canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if self.docs.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("No captures yet. Use a button above or a hot key.");
            });
            return;
        }
        if self.tex_dirty {
            let flat = self.docs[self.current].flatten();
            let ci = egui::ColorImage::from_rgba_unmultiplied([flat.width() as usize, flat.height() as usize], flat.as_raw());
            self.tex = Some(ctx.load_texture("doc", ci, egui::TextureOptions::NEAREST));
            self.tex_dirty = false;
        }
        let ppp = ctx.pixels_per_point();
        let tex = self.tex.clone().unwrap();
        let (iw, ih) = self.docs[self.current].image().dimensions();
        let size = egui::vec2(iw as f32 / ppp, ih as f32 / ppp);

        egui::ScrollArea::both().show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let to_img = |p: Pos2| -> Pt { ((p.x - rect.min.x) * ppp, (p.y - rect.min.y) * ppp) };
            let to_ui = |p: Pt| Pos2::new(rect.min.x + p.0 / ppp, rect.min.y + p.1 / ppp);
            let color = self.color;
            let rgba = [color.r(), color.g(), color.b(), color.a()];

            if self.tool == Tool::Text {
                if resp.clicked() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        self.pending_text = Some((to_img(p), String::new()));
                    }
                }
            } else {
                if resp.drag_started() {
                    if let Some(p) = resp.interact_pointer_pos() {
                        self.drag = Some(vec![to_img(p)]);
                    }
                }
                if resp.dragged() {
                    if let (Some(path), Some(p)) = (self.drag.as_mut(), resp.interact_pointer_pos()) {
                        let q = to_img(p);
                        if self.tool == Tool::Pencil {
                            path.push(q);
                        } else {
                            path.truncate(1);
                            path.push(q);
                        }
                    }
                }
                // Live preview of the shape being dragged.
                if let Some(path) = &self.drag {
                    let stroke = Stroke::new(self.width / ppp, color);
                    let pts: Vec<Pos2> = path.iter().map(|p| to_ui(*p)).collect();
                    match self.tool {
                        Tool::Pencil => {
                            ui.painter().add(egui::Shape::line(pts, stroke));
                        }
                        Tool::Line | Tool::Arrow if pts.len() == 2 => {
                            ui.painter().line_segment([pts[0], pts[1]], stroke);
                        }
                        Tool::Rect | Tool::Crop if pts.len() == 2 => {
                            let r = egui::Rect::from_two_pos(pts[0], pts[1]);
                            let st = if self.tool == Tool::Crop { Stroke::new(1.5, Color32::YELLOW) } else { stroke };
                            ui.painter().rect_stroke(r, 0.0, st, egui::StrokeKind::Middle);
                        }
                        Tool::Ellipse if pts.len() == 2 => {
                            let r = egui::Rect::from_two_pos(pts[0], pts[1]);
                            ui.painter().add(egui::epaint::EllipseShape::stroke(r.center(), r.size() / 2.0, stroke));
                        }
                        _ => {}
                    }
                }
                if resp.drag_stopped() {
                    if let Some(path) = self.drag.take() {
                        let (w, tool) = (self.width, self.tool);
                        let kind = match (tool, path.as_slice()) {
                            (Tool::Pencil, p) if !p.is_empty() => Some(Kind::Pencil(p.to_vec())),
                            (Tool::Line, [a, b]) => Some(Kind::Line(*a, *b)),
                            (Tool::Arrow, [a, b]) => Some(Kind::Arrow(*a, *b)),
                            (Tool::Rect, [a, b]) => Some(Kind::Rect(*a, *b)),
                            (Tool::Ellipse, [a, b]) => Some(Kind::Ellipse(*a, *b)),
                            (Tool::Crop, [a, b]) => {
                                let (x, y) = (a.0.min(b.0).max(0.0) as u32, a.1.min(b.1).max(0.0) as u32);
                                let (cw, ch) = ((a.0 - b.0).abs() as u32, (a.1 - b.1).abs() as u32);
                                self.docs[self.current].crop(x, y, cw, ch);
                                self.tex_dirty = true;
                                None
                            }
                            _ => None,
                        };
                        if let Some(kind) = kind {
                            self.docs[self.current].add_shape(Shape { kind, color: rgba, width: w });
                            self.tex_dirty = true;
                        }
                    }
                }
            }
        });

        let mut commit = None;
        if let Some((pos, text)) = &mut self.pending_text {
            let mut open = true;
            egui::Window::new("Text").open(&mut open).collapsible(false).show(ctx, |ui| {
                let r = ui.text_edit_singleline(text);
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    commit = Some((*pos, text.clone()));
                }
            });
            if !open {
                self.pending_text = None;
            }
        }
        if let Some((pos, text)) = commit {
            self.pending_text = None;
            if !text.is_empty() {
                let rgba = [self.color.r(), self.color.g(), self.color.b(), self.color.a()];
                self.docs[self.current].add_shape(Shape { kind: Kind::Text(pos, text), color: rgba, width: (self.width / 3.0).max(2.0) });
                self.tex_dirty = true;
            }
        }
    }
}
