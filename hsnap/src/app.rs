//! Main window. Layout and menu structure follow the HyperSnap 6 manual:
//! menu bar, main toolbar, drawing palette on the left, tabbed documents, status bar.

use crate::capture::Rect as PxRect;
use crate::dialogs::{self, Dialog};
use crate::editor::{self, Document, Kind, Pt, Shape};
use crate::hotkeys::Hotkeys;
use crate::icons;
use crate::quicksave::QuickSave;
use crate::service::{CaptureMode, Captured, Service, Settings, ViewEdit};
use eframe::egui::{self, Color32, Key, Modifiers, Pos2, Sense, Stroke};
use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tool {
    Select,
    FloodFill,
    Text,
    Airbrush,
    Arrow,
    Eraser,
    Freehand,
    Line,
    Rect,
    RoundRect,
    Ellipse,
    FilledRect,
    FilledRoundRect,
    FilledEllipse,
    Magnify,
    ViewOnly,
}

/// Palette order and tooltip text, following the manual's drawing-toolbar page.
const PALETTE: &[(Tool, &str)] = &[
    (Tool::Select, "Select area"),
    (Tool::FloodFill, "Flood fill"),
    (Tool::Text, "Add text"),
    (Tool::Airbrush, "Air brush"),
    (Tool::Arrow, "Arrow"),
    (Tool::Eraser, "Eraser"),
    (Tool::Freehand, "Freehand draw"),
    (Tool::Line, "Line"),
    (Tool::Rect, "Rectangle"),
    (Tool::RoundRect, "Rounded rectangle"),
    (Tool::Ellipse, "Ellipse (Shift = circle)"),
    (Tool::FilledRect, "Filled rectangle"),
    (Tool::FilledRoundRect, "Filled rounded rectangle"),
    (Tool::FilledEllipse, "Filled ellipse"),
    (Tool::Magnify, "Magnify (left = in, right = out)"),
    (Tool::ViewOnly, "View only (drag to pan)"),
];

#[derive(Clone, Copy, PartialEq)]
pub enum Zoom {
    Auto,
    Percent(f32),
}

pub struct App {
    pub svc: Service,
    captures: Receiver<Captured>,
    msgs: Receiver<String>,
    msg_tx: std::sync::mpsc::Sender<String>,
    hotkeys: Hotkeys,
    pub docs: Vec<Document>,
    pub current: usize,
    tex: Option<egui::TextureHandle>,
    tex_dirty: bool,
    tool: Tool,
    pub fg: Color32,
    pub bg: Color32,
    width: f32,
    eraser_width: f32,
    zoom: Zoom,
    drag: Option<Vec<Pt>>,
    selection: Option<PxRect>,
    pending_text: Option<(Pt, String)>,
    pub quicksave: Option<QuickSave>,
    counter: u32,
    pub status: String,
    last_img: Option<RgbaImage>,
    hover_px: Option<(i32, i32)>,
    show_main_toolbar: bool,
    show_palette: bool,
    show_status: bool,
    pub dialog: Dialog,
    rng: u64,
    ctx: egui::Context,
    tray: Option<crate::tray::Tray>,
    quitting: Arc<AtomicBool>,
    first_frame: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        let (msg_tx, msgs) = std::sync::mpsc::channel();
        let settings = Settings::load();
        let svc = Service::new(tx, cc.egui_ctx.clone(), settings.clone());
        let ctx = cc.egui_ctx.clone();
        let binds = if settings.hotkeys_enabled { settings.hotkeys.clone() } else { vec![] };
        let hotkeys = Hotkeys::start(binds, move || ctx.request_repaint());
        let bgc = settings.background;
        Self {
            svc,
            captures: rx,
            msgs,
            msg_tx,
            hotkeys,
            docs: vec![],
            current: 0,
            tex: None,
            tex_dirty: true,
            tool: Tool::Rect,
            fg: Color32::from_rgb(230, 30, 30),
            bg: Color32::from_rgba_unmultiplied(bgc[0], bgc[1], bgc[2], bgc[3]),
            width: 3.0,
            eraser_width: 12.0,
            zoom: Zoom::Percent(100.0),
            drag: None,
            selection: None,
            pending_text: None,
            quicksave: None,
            counter: 0,
            status: "Ready".into(),
            last_img: None,
            hover_px: None,
            show_main_toolbar: true,
            show_palette: true,
            show_status: true,
            dialog: Dialog::None,
            rng: 0x2545F4914F6CDD1D,
            ctx: cc.egui_ctx.clone(),
            tray: None,
            quitting: Arc::new(AtomicBool::new(false)),
            first_frame: true,
        }
    }

    /// Create or drop the tray icon to match the settings.
    fn sync_tray(&mut self) {
        let want = self.settings().show_tray;
        if want && self.tray.is_none() {
            match crate::tray::Tray::new(self.svc.clone(), self.ctx.clone(), self.quitting.clone()) {
                Ok(t) => self.tray = Some(t),
                Err(e) => self.status = format!("Tray icon failed: {e}"),
            }
        } else if !want {
            self.tray = None;
        }
    }

    pub fn push_recent(&mut self, path: &std::path::Path) {
        let mut s = self.settings();
        let p = path.to_string_lossy().into_owned();
        s.recent.retain(|r| *r != p);
        s.recent.insert(0, p);
        s.recent.truncate(s.recent_count as usize);
        self.apply_settings_quiet(s);
    }

    /// Persist settings without touching hot keys, tray or QuickSave numbering.
    pub fn apply_settings_quiet(&mut self, s: Settings) {
        s.save();
        *self.svc.settings.lock().unwrap() = s;
    }

    /// Print the current image: with the print dialog, or straight to the default printer.
    pub fn print_document(&mut self, direct: bool) {
        let Some(doc) = self.docs.get(self.current) else { return };
        let (img, name, setup) = (doc.flatten(), doc.name.clone(), self.settings().page_setup);
        self.spawn_print(img, name, setup, direct);
    }

    fn spawn_print(&self, img: RgbaImage, name: String, setup: crate::print::PageSetup, direct: bool) {
        let (tx, ctx) = (self.msg_tx.clone(), self.ctx.clone());
        std::thread::spawn(move || {
            let target = if direct { crate::print_gdi::Target::DefaultPrinter } else { crate::print_gdi::Target::Dialog(None) };
            let msg = match crate::print_gdi::print_image(&img, &setup, &name, target) {
                Ok(()) => format!("Printed {name}"),
                Err(e) if e == "cancelled" => "Print cancelled".to_string(),
                Err(e) => format!("Print failed: {e}"),
            };
            let _ = tx.send(msg);
            ctx.request_repaint();
        });
    }

    pub fn settings(&self) -> Settings {
        self.svc.settings.lock().unwrap().clone()
    }

    /// Store new settings, persist them and re-register hot keys.
    pub fn apply_settings(&mut self, s: Settings) {
        s.save();
        self.hotkeys.set(if s.hotkeys_enabled { s.hotkeys.clone() } else { vec![] });
        *self.svc.settings.lock().unwrap() = s;
        self.quicksave = None; // restart numbering with the new template
        self.sync_tray();
    }

    pub fn hotkey_status(&self) -> Vec<(crate::hotkeys::HotkeyAction, bool)> {
        self.hotkeys.registered.lock().unwrap().clone()
    }

    fn on_capture(&mut self, mut c: Captured) {
        let s = self.settings();
        let store = crate::stamps::Store { dir: std::path::PathBuf::from(&s.stamps_dir) };
        for st in store.list().into_iter().filter(|st| st.auto_add) {
            let pic = store.picture(&st);
            let m = crate::stamps::MacroCtx::now(c.label, "");
            c.img = crate::stamps::apply(&c.img, &st, pic.as_ref(), &m, None);
        }
        if s.only_if_changed && self.last_img.as_ref() == Some(&c.img) {
            return;
        }
        self.last_img = Some(c.img.clone());
        if s.auto_print {
            self.spawn_print(c.img.clone(), c.label.to_string(), s.page_setup.clone(), !s.auto_print_prompt);
        }
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
                    self.status = match c.img.save(&path) {
                        Ok(()) => format!("Saved {}", path.display()),
                        Err(e) => format!("Save failed: {e}"),
                    };
                    name = Some(n);
                }
                None => self.status = "QuickSave reached its stop number.".into(),
            }
        } else {
            self.status = format!("{} captured ({}x{})", c.label, c.img.width(), c.img.height());
        }
        match s.view_edit {
            ViewEdit::DoNotChange => {}
            ViewEdit::Replace if !self.docs.is_empty() => {
                let d = &mut self.docs[self.current];
                d.transform(|_| c.img);
                d.modified = s.mark_modified;
                self.tex_dirty = true;
            }
            _ => {
                self.counter += 1;
                let name = name.unwrap_or_else(|| format!("Capture {}", self.counter));
                let mut d = Document::new(name, c.img);
                d.modified = s.mark_modified;
                self.docs.push(d);
                self.current = self.docs.len() - 1;
                self.selection = None;
                self.tex_dirty = true;
            }
        }
    }

    pub fn new_document(&mut self, img: RgbaImage, name: String) {
        self.docs.push(Document::new(name, img));
        self.current = self.docs.len() - 1;
        self.selection = None;
        self.tex_dirty = true;
    }

    /// The selected fragment of the current image (or the whole image when nothing is selected).
    pub fn selection_fragment(&self) -> Option<RgbaImage> {
        let (x, y, w, h) = self.region()?;
        let flat = self.docs.get(self.current)?.flatten();
        Some(image::imageops::crop_imm(&flat, x, y, w, h).to_image())
    }

    /// Put a stamp on the current image.
    pub fn insert_stamp(&mut self, stamp: &crate::stamps::Stamp, picture: Option<&RgbaImage>) {
        let Some(doc) = self.docs.get(self.current) else {
            self.status = "Open or capture an image first".into();
            return;
        };
        let path = doc.path.as_ref().map(|p| p.display().to_string()).unwrap_or_default();
        let m = crate::stamps::MacroCtx::now(&doc.name, &path);
        let sel = if self.selection.is_some() { self.region() } else { None };
        let (st, pic) = (stamp.clone(), picture.cloned());
        self.edit(|d| d.transform(|img| crate::stamps::apply(&img, &st, pic.as_ref(), &m, sel)));
        self.selection = None;
        self.status = format!("Inserted stamp {}", stamp.name);
    }

    fn preview_for_current(&self) -> crate::print_ui::Preview {
        match self.docs.get(self.current) {
            Some(d) => crate::print_ui::Preview::new(d.flatten(), d.name.clone()),
            None => crate::print_ui::Preview::placeholder(),
        }
    }

    fn open_print_preview(&mut self) {
        let prev = self.preview_for_current();
        self.dialog = Dialog::PrintPreview { prev: Box::new(prev), page: 0 };
    }

    fn open_page_setup(&mut self) {
        let prev = self.preview_for_current();
        self.dialog = Dialog::PageSetup { draft: Box::new(self.settings().page_setup), prev: Box::new(prev) };
    }

    fn has_doc(&self) -> bool {
        !self.docs.is_empty()
    }

    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Document) -> R) -> Option<R> {
        let r = self.docs.get_mut(self.current).map(f);
        self.tex_dirty = true;
        r
    }

    fn open_file(&mut self) {
        let Some(path) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif", "tif", "tiff", "webp"]).pick_file() else { return };
        self.open_path(path);
    }

    fn open_path(&mut self, path: std::path::PathBuf) {
        match image::open(&path) {
            Ok(img) => {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.new_document(img.to_rgba8(), name);
                self.docs[self.current].path = Some(path.clone());
                self.push_recent(&path);
            }
            Err(e) => self.status = format!("Open failed: {e}"),
        }
    }

    fn save_as(&mut self) {
        if !self.has_doc() {
            return;
        }
        let name = self.docs[self.current].name.clone();
        let stem = std::path::Path::new(&name).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or(name);
        let Some(path) = rfd::FileDialog::new()
            .set_file_name(format!("{stem}.png"))
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .add_filter("BMP", &["bmp"])
            .add_filter("GIF", &["gif"])
            .save_file()
        else {
            return;
        };
        self.save_to(path);
    }

    fn save(&mut self) {
        match self.docs.get(self.current).and_then(|d| d.path.clone()) {
            Some(p) => self.save_to(p),
            None => self.save_as(),
        }
    }

    fn save_to(&mut self, path: std::path::PathBuf) {
        let flat = self.docs[self.current].flatten();
        let res = if matches!(path.extension().and_then(|e| e.to_str()), Some("jpg" | "jpeg")) {
            image::DynamicImage::ImageRgba8(flat).to_rgb8().save(&path) // JPEG has no alpha
        } else {
            flat.save(&path)
        };
        self.status = match res {
            Ok(()) => {
                let d = &mut self.docs[self.current];
                d.modified = false;
                d.path = Some(path.clone());
                d.name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                self.push_recent(&path);
                format!("Saved {}", path.display())
            }
            Err(e) => format!("Save failed: {e}"),
        };
    }

    fn close_current(&mut self) {
        if self.has_doc() {
            self.docs.remove(self.current);
            self.current = self.current.min(self.docs.len().saturating_sub(1));
            self.selection = None;
            self.tex_dirty = true;
        }
    }

    fn bg_rgba(&self) -> [u8; 4] {
        [self.bg.r(), self.bg.g(), self.bg.b(), self.bg.a()]
    }
    fn fg_rgba(&self) -> [u8; 4] {
        [self.fg.r(), self.fg.g(), self.fg.b(), self.fg.a()]
    }

    /// Selection clamped to the image, or the whole image.
    fn region(&self) -> Option<(u32, u32, u32, u32)> {
        let d = self.docs.get(self.current)?;
        let (iw, ih) = d.image().dimensions();
        match self.selection {
            Some(s) if s.w > 0 && s.h > 0 => {
                let x = s.x.clamp(0, iw as i32 - 1) as u32;
                let y = s.y.clamp(0, ih as i32 - 1) as u32;
                Some((x, y, (s.w as u32).min(iw - x), (s.h as u32).min(ih - y)))
            }
            _ => Some((0, 0, iw, ih)),
        }
    }

    fn copy(&mut self) {
        if let Some((x, y, w, h)) = self.region() {
            let flat = self.docs[self.current].flatten();
            copy_to_clipboard(&image::imageops::crop_imm(&flat, x, y, w, h).to_image());
            self.status = "Copied to clipboard".into();
        }
    }

    fn cut(&mut self) {
        self.copy();
        self.clear_region();
    }

    fn clear_region(&mut self) {
        if let Some((x, y, w, h)) = self.region() {
            let bg = self.bg_rgba();
            self.edit(|d| d.transform(|mut i| {
                editor::fill_rect(&mut i, x, y, w, h, bg);
                i
            }));
        }
    }

    fn clipboard_image() -> Option<RgbaImage> {
        let img = arboard::Clipboard::new().ok()?.get_image().ok()?;
        RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())
    }

    fn paste(&mut self) {
        let Some(src) = Self::clipboard_image() else {
            self.status = "Clipboard has no image".into();
            return;
        };
        let (px, py) = self.selection.map(|s| (s.x as i64, s.y as i64)).unwrap_or((0, 0));
        self.edit(|d| d.transform(|mut i| {
            editor::paste(&mut i, &src, px, py);
            i
        }));
    }

    fn paste_as_new(&mut self) {
        match Self::clipboard_image() {
            Some(img) => {
                self.counter += 1;
                let n = format!("Pasted {}", self.counter);
                self.new_document(img, n);
            }
            None => self.status = "Clipboard has no image".into(),
        }
    }

    fn set_wallpaper(&mut self) {
        if !self.has_doc() {
            return;
        }
        let path = std::env::temp_dir().join("hsnap-wallpaper.bmp");
        let flat = image::DynamicImage::ImageRgba8(self.docs[self.current].flatten()).to_rgb8();
        if flat.save(&path).is_err() {
            self.status = "Could not write wallpaper file".into();
            return;
        }
        use windows::Win32::UI::WindowsAndMessaging::*;
        let mut wide: Vec<u16> = path.to_string_lossy().encode_utf16().chain(Some(0)).collect();
        let ok = unsafe { SystemParametersInfoW(SPI_SETDESKWALLPAPER, 0, Some(wide.as_mut_ptr() as *mut _), SPIF_UPDATEINIFILE | SPIF_SENDCHANGE).is_ok() };
        self.status = if ok { "Wallpaper set".into() } else { "Setting wallpaper failed".into() };
    }

    fn shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let pressed = |ctx: &egui::Context, m: Modifiers, k: Key| ctx.input_mut(|i| i.consume_key(m, k));
        if pressed(ctx, cmd, Key::N) {
            self.dialog = Dialog::New { w: 800, h: 600 };
        }
        if pressed(ctx, cmd, Key::O) {
            self.open_file();
        }
        if pressed(ctx, cmd, Key::S) {
            self.save();
        }
        if pressed(ctx, cmd | Modifiers::SHIFT, Key::Z) || pressed(ctx, cmd, Key::Y) {
            self.edit(|d| d.redo());
        }
        if pressed(ctx, cmd, Key::Z) {
            self.edit(|d| d.undo());
        }
        if pressed(ctx, cmd, Key::C) {
            self.copy();
        }
        if pressed(ctx, cmd, Key::X) {
            self.cut();
        }
        if pressed(ctx, cmd, Key::V) {
            self.paste();
        }
        if pressed(ctx, Modifiers::NONE, Key::Delete) {
            self.clear_region();
        }
    }
}

pub fn copy_to_clipboard(img: &RgbaImage) {
    if let Ok(mut cb) = arboard::Clipboard::new() {
        let _ = cb.set_image(arboard::ImageData { width: img.width() as usize, height: img.height() as usize, bytes: std::borrow::Cow::Borrowed(img.as_raw()) });
    }
}

fn todo_item(ui: &mut egui::Ui, label: &str) {
    ui.add_enabled(false, egui::Button::new(label)).on_disabled_hover_text("Not implemented yet");
}

impl eframe::App for App {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        if self.first_frame {
            self.first_frame = false;
            self.sync_tray();
            // Developer hook: open a dialog on start-up (used for visual checks).
            match std::env::var("HSNAP_DIALOG").as_deref() {
                Ok("pagesetup") => self.open_page_setup(),
                Ok("stamps") => self.dialog = Dialog::Stamps(Box::new(crate::stamps_ui::Manager::new(std::path::PathBuf::from(self.settings().stamps_dir)))),
                Ok("stampedit") => self.dialog = Dialog::Stamps(Box::new(crate::stamps_ui::Manager::with_new_editor(std::path::PathBuf::from(self.settings().stamps_dir)))),
                Ok("preview") => {
                    let p = crate::print_ui::Preview::placeholder();
                    self.dialog = Dialog::PrintPreview { prev: Box::new(p), page: 0 };
                }
                _ => {}
            }
            if self.settings().start_minimized {
                let cmd = if self.tray.is_some() { egui::ViewportCommand::Visible(false) } else { egui::ViewportCommand::Minimized(true) };
                ctx.send_viewport_cmd(cmd);
            }
        }
        if ctx.input(|i| i.viewport().close_requested()) {
            let s = self.settings();
            if s.show_tray && s.keep_in_tray && self.tray.is_some() && !self.quitting.load(Ordering::Relaxed) {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                crate::service::show_own(windows::Win32::UI::WindowsAndMessaging::SW_HIDE);
            }
        }
        while let Ok(a) = self.hotkeys.events.try_recv() {
            self.svc.handle_hotkey(a);
        }
        while let Ok(m) = self.msgs.try_recv() {
            self.status = m;
        }
        while let Ok(c) = self.captures.try_recv() {
            self.on_capture(c);
        }
        if !ctx.wants_keyboard_input() {
            self.shortcuts(ctx);
        }

        egui::TopBottomPanel::top("menu").show(ctx, |ui| self.menu_bar(ui, ctx));
        if self.show_main_toolbar {
            egui::TopBottomPanel::top("toolbar").show(ctx, |ui| self.main_toolbar(ui));
        }
        if self.show_status {
            egui::TopBottomPanel::bottom("status").show(ctx, |ui| self.status_bar(ui));
        }
        egui::TopBottomPanel::bottom("tabs").show(ctx, |ui| self.tabs(ui));
        if self.show_palette {
            egui::SidePanel::left("palette").resizable(false).exact_width(104.0).show(ctx, |ui| self.palette(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(ctx.style().visuals.extreme_bg_color)).show(ctx, |ui| self.canvas(ui, ctx));

        dialogs::show(self, ctx);
    }
}

impl App {
    fn capture_item(&mut self, ui: &mut egui::Ui, label: &str, mode: CaptureMode) {
        if ui.button(label).clicked() {
            ui.close_menu();
            self.svc.capture(mode);
        }
    }

    fn menu_bar(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let has = self.has_doc();
        egui::menu::bar(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New…\tCtrl+N").clicked() {
                    ui.close_menu();
                    self.dialog = Dialog::New { w: 800, h: 600 };
                }
                if ui.button("Open…\tCtrl+O").clicked() {
                    ui.close_menu();
                    self.open_file();
                }
                if ui.add_enabled(has, egui::Button::new("Save\tCtrl+S")).clicked() {
                    ui.close_menu();
                    self.save();
                }
                if ui.add_enabled(has, egui::Button::new("Save As…")).clicked() {
                    ui.close_menu();
                    self.save_as();
                }
                todo_item(ui, "Upload to FTP Server…");
                if ui.add_enabled(has, egui::Button::new("Close")).clicked() {
                    ui.close_menu();
                    self.close_current();
                }
                ui.separator();
                todo_item(ui, "Acquire");
                todo_item(ui, "Select Source…");
                ui.separator();
                if ui.add_enabled(has, egui::Button::new("Print…")).clicked() {
                    ui.close_menu();
                    self.print_document(false);
                }
                if ui.add_enabled(has, egui::Button::new("Print Preview")).clicked() {
                    ui.close_menu();
                    self.open_print_preview();
                }
                if ui.button("Page Setup…").clicked() {
                    ui.close_menu();
                    self.open_page_setup();
                }
                todo_item(ui, "Send by Email…");
                if ui.add_enabled(has, egui::Button::new("Set as Wallpaper")).clicked() {
                    ui.close_menu();
                    self.set_wallpaper();
                }
                let recent = self.settings().recent;
                if !recent.is_empty() {
                    ui.separator();
                    for (i, r) in recent.iter().enumerate() {
                        if ui.button(format!("{} {}", i + 1, r)).clicked() {
                            ui.close_menu();
                            self.open_path(std::path::PathBuf::from(r));
                        }
                    }
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
            ui.menu_button("Edit", |ui| {
                let (u, r) = self.docs.get(self.current).map_or((false, false), |d| (d.can_undo(), d.can_redo()));
                if ui.add_enabled(u, egui::Button::new("Undo\tCtrl+Z")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.undo());
                }
                if ui.add_enabled(r, egui::Button::new("Redo\tCtrl+Y")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.redo());
                }
                ui.separator();
                if ui.add_enabled(has, egui::Button::new("Cut\tCtrl+X")).clicked() {
                    ui.close_menu();
                    self.cut();
                }
                if ui.add_enabled(has, egui::Button::new("Copy\tCtrl+C")).clicked() {
                    ui.close_menu();
                    self.copy();
                }
                if ui.add_enabled(has, egui::Button::new("Paste\tCtrl+V")).clicked() {
                    ui.close_menu();
                    self.paste();
                }
                if ui.button("Paste as New Image").clicked() {
                    ui.close_menu();
                    self.paste_as_new();
                }
                todo_item(ui, "Copy Format");
            });
            ui.menu_button("View", |ui| {
                if ui.button("Full Screen").clicked() {
                    ui.close_menu();
                    let on = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(!on));
                }
                ui.separator();
                if ui.add_enabled(self.docs.len() > 1, egui::Button::new("Previous File")).clicked() {
                    ui.close_menu();
                    self.current = (self.current + self.docs.len() - 1) % self.docs.len();
                    self.tex_dirty = true;
                }
                if ui.add_enabled(self.docs.len() > 1, egui::Button::new("Next File")).clicked() {
                    ui.close_menu();
                    self.current = (self.current + 1) % self.docs.len();
                    self.tex_dirty = true;
                }
                ui.separator();
                ui.checkbox(&mut self.show_main_toolbar, "Toolbar");
                ui.checkbox(&mut self.show_status, "Status Bar");
                ui.checkbox(&mut self.show_palette, "Drawing Tools");
                ui.separator();
                todo_item(ui, "Customize…");
            });
            ui.menu_button("Capture", |ui| {
                self.capture_item(ui, "Full Screen", CaptureMode::Full);
                self.capture_item(ui, "Virtual Desktop", CaptureMode::Virtual);
                self.capture_item(ui, "Window or Control", CaptureMode::Window);
                self.capture_item(ui, "Entire Page with Scrolling	Ctrl+Shift+S", CaptureMode::Scroll);
                todo_item(ui, "Button");
                self.capture_item(ui, "Active Window", CaptureMode::Active);
                self.capture_item(ui, "Active Window without Frame", CaptureMode::ActiveNoFrame);
                ui.separator();
                self.capture_item(ui, "Region", CaptureMode::Region);
                self.capture_item(ui, "Free Hand", CaptureMode::FreeHand);
                self.capture_item(ui, "Pan Last Region", CaptureMode::Pan);
                self.capture_item(ui, "Multi-Region	Ctrl+Shift+M", CaptureMode::Multi);
                ui.separator();
                self.capture_item(ui, "Mouse Cursor Only", CaptureMode::Cursor);
                if ui.button("Repeat Last Capture").clicked() {
                    ui.close_menu();
                    self.svc.repeat_last();
                }
                todo_item(ui, "Extended Active Window");
                todo_item(ui, "Enable Video or Game Capture");
                ui.separator();
                if ui.button("Capture Settings…").clicked() {
                    ui.close_menu();
                    self.dialog = Dialog::CaptureSettings { tab: 0, draft: Box::new(self.settings()) };
                }
                if ui.button("Screen Capture Hot Keys…").clicked() {
                    ui.close_menu();
                    self.dialog = Dialog::HotKeys { draft: Box::new(self.settings()), recording: None };
                }
            });
            ui.menu_button("Image", |ui| {
                if ui.add_enabled(has, egui::Button::new("Crop")).clicked() {
                    ui.close_menu();
                    if let (Some((x, y, w, h)), true) = (self.region(), self.selection.is_some()) {
                        self.edit(|d| d.crop(x, y, w, h));
                        self.selection = None;
                    }
                }
                todo_item(ui, "Cut out strip…");
                ui.separator();
                if ui.add_enabled(has, egui::Button::new("Scale…")).clicked() {
                    ui.close_menu();
                    if let Some(d) = self.docs.get(self.current) {
                        let (w, h) = d.image().dimensions();
                        self.dialog = Dialog::Scale { w, h, keep: true };
                    }
                }
                todo_item(ui, "Change Resolution…");
                if ui.add_enabled(has, egui::Button::new("Auto-trim")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.transform(editor::auto_trim));
                    self.selection = None;
                }
                ui.separator();
                if ui.button("Stamps…").clicked() {
                    ui.close_menu();
                    let dir = std::path::PathBuf::from(self.settings().stamps_dir);
                    self.dialog = Dialog::Stamps(Box::new(crate::stamps_ui::Manager::new(dir)));
                }
                if ui.add_enabled(has, egui::Button::new("Mirror (horizontal)")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.flip_horizontal());
                }
                if ui.add_enabled(has, egui::Button::new("Mirror (vertical)")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.transform(|i| image::imageops::flip_vertical(&i)));
                }
                ui.menu_button("Rotate", |ui| {
                    if ui.add_enabled(has, egui::Button::new("90° clockwise")).clicked() {
                        ui.close_menu();
                        self.edit(|d| d.rotate90());
                        self.selection = None;
                    }
                    if ui.add_enabled(has, egui::Button::new("180°")).clicked() {
                        ui.close_menu();
                        self.edit(|d| d.transform(|i| image::imageops::rotate180(&i)));
                    }
                    if ui.add_enabled(has, egui::Button::new("90° counter-clockwise")).clicked() {
                        ui.close_menu();
                        self.edit(|d| d.transform(|i| image::imageops::rotate270(&i)));
                        self.selection = None;
                    }
                });
                todo_item(ui, "Shear…");
                todo_item(ui, "Mosaic…");
                todo_item(ui, "Emboss…");
                todo_item(ui, "Sharpen or Blur…");
                todo_item(ui, "Shadow…");
                todo_item(ui, "Frame…");
            });
            ui.menu_button("Color", |ui| {
                todo_item(ui, "Color Resolution…");
                if ui.add_enabled(has, egui::Button::new("Black and White")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.transform(editor::black_and_white));
                }
                todo_item(ui, "Halftone…");
                if ui.add_enabled(has, egui::Button::new("Gray Scale")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.transform(editor::grayscale));
                }
                todo_item(ui, "Color Correction…");
                todo_item(ui, "Invert Black and White");
                if ui.add_enabled(has, egui::Button::new("Invert Colors")).clicked() {
                    ui.close_menu();
                    self.edit(|d| d.transform(editor::invert));
                }
                todo_item(ui, "Substitute Colors…");
                todo_item(ui, "Unique Colors");
            });
            ui.menu_button("TextSnap", |ui| {
                todo_item(ui, "Text from a Region");
                todo_item(ui, "Text from an Object under Cursor");
                todo_item(ui, "Text from a window with auto-scroll");
                todo_item(ui, "Text from a region with auto-scroll");
                ui.separator();
                todo_item(ui, "TextSnap Settings…");
                todo_item(ui, "TextSnap Hot Keys…");
            });
            ui.menu_button("Options", |ui| {
                let mut s = self.settings();
                if ui.checkbox(&mut s.hotkeys_enabled, "Activate Hot Keys").changed() {
                    self.apply_settings(s);
                }
                todo_item(ui, "Activate Voice Commands");
                todo_item(ui, "Control Mouse with Keyboard");
                if ui.button("Startup and Tray Icon…").clicked() {
                    ui.close_menu();
                    let mut d = self.settings();
                    d.autostart = crate::startup::is_enabled("HSnap", d.autostart_all_users); // trust the registry
                    self.dialog = Dialog::Startup { draft: Box::new(d), error: None };
                }
            });
            ui.menu_button("Window", |ui| {
                if ui.add_enabled(has, egui::Button::new("Close All")).clicked() {
                    ui.close_menu();
                    self.docs.clear();
                    self.current = 0;
                    self.tex_dirty = true;
                }
            });
            ui.menu_button("Help", |ui| {
                if ui.button("Contents (original manual)").clicked() {
                    ui.close_menu();
                    let chm = r"C:\Program Files (x86)\HyperSnap 6\HprSnap6Man.chm";
                    if std::path::Path::new(chm).exists() {
                        let _ = std::process::Command::new("hh.exe").arg(chm).spawn();
                    } else {
                        self.status = "HyperSnap 6 manual not found".into();
                    }
                }
                if ui.button("About HSnap").clicked() {
                    ui.close_menu();
                    self.dialog = Dialog::About;
                }
            });
        });
    }

    fn main_toolbar(&mut self, ui: &mut egui::Ui) {
        let has = self.has_doc();
        ui.horizontal_wrapped(|ui| {
            if ui.button("New").clicked() {
                self.dialog = Dialog::New { w: 800, h: 600 };
            }
            if ui.button("Open").clicked() {
                self.open_file();
            }
            if ui.add_enabled(has, egui::Button::new("Save")).clicked() {
                self.save();
            }
            if ui.add_enabled(has, egui::Button::new("Print Direct")).on_hover_text("Print to the default printer without the dialog").clicked() {
                self.print_document(true);
            }
            ui.separator();
            for (label, mode) in [("Full Screen", CaptureMode::Full), ("Window", CaptureMode::Window), ("Active", CaptureMode::Active), ("Region", CaptureMode::Region), ("Cursor", CaptureMode::Cursor)] {
                if ui.button(label).clicked() {
                    self.svc.capture(mode);
                }
            }
            if ui.button("Repeat").clicked() {
                self.svc.repeat_last();
            }
            if !self.svc.is_auto_running() && ui.button("Auto-capture").on_hover_text("Repeat the last capture every N seconds (set in Capture Settings > QuickSave); stop with Shift+F11").clicked() {
                let secs = self.settings().repeat_secs;
                self.svc.start_auto(secs);
            }
            ui.separator();
            let (u, r) = self.docs.get(self.current).map_or((false, false), |d| (d.can_undo(), d.can_redo()));
            if ui.add_enabled(u, egui::Button::new("Undo")).clicked() {
                self.edit(|d| d.undo());
            }
            if ui.add_enabled(r, egui::Button::new("Redo")).clicked() {
                self.edit(|d| d.redo());
            }
            ui.separator();
            ui.label("Zoom");
            let label = match self.zoom {
                Zoom::Auto => "Auto".to_string(),
                Zoom::Percent(p) => format!("{}%", p.round() as i32),
            };
            egui::ComboBox::from_id_salt("zoom").width(70.0).selected_text(label).show_ui(ui, |ui| {
                ui.selectable_value(&mut self.zoom, Zoom::Auto, "Auto");
                for p in [25.0, 50.0, 75.0, 100.0, 150.0, 200.0, 400.0] {
                    ui.selectable_value(&mut self.zoom, Zoom::Percent(p), format!("{}%", p as i32));
                }
            });
            ui.separator();
            if ui.button("Capture Settings").clicked() {
                self.dialog = Dialog::CaptureSettings { tab: 0, draft: Box::new(self.settings()) };
            }
            if self.svc.is_auto_running() && ui.button("Stop auto-capture").clicked() {
                self.svc.stop_auto();
            }
        });
    }

    fn palette(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        let has = self.has_doc();
        egui::Grid::new("pal-edit").spacing([2.0, 2.0]).show(ui, |ui| {
            let sm = |ui: &mut egui::Ui, t: &str, on: bool| ui.add_enabled(on, egui::Button::new(t).min_size(egui::vec2(34.0, 22.0)));
            let (u, r) = self.docs.get(self.current).map_or((false, false), |d| (d.can_undo(), d.can_redo()));
            if sm(ui, "Cut", has).clicked() { self.cut(); }
            if sm(ui, "Copy", has).clicked() { self.copy(); }
            ui.end_row();
            if sm(ui, "Paste", has).clicked() { self.paste(); }
            if sm(ui, "Clear", has).clicked() { self.clear_region(); }
            ui.end_row();
            if sm(ui, "Undo", u).clicked() { self.edit(|d| d.undo()); }
            if sm(ui, "Redo", r).clicked() { self.edit(|d| d.redo()); }
            ui.end_row();
        });
        ui.separator();
        egui::Grid::new("pal-tools").spacing([2.0, 2.0]).show(ui, |ui| {
            for (i, (tool, hint)) in PALETTE.iter().enumerate() {
                if icons::tool_button(ui, *tool, self.tool == *tool, hint).clicked() {
                    self.tool = *tool;
                    if *tool != Tool::Select {
                        self.selection = None;
                    }
                }
                if i % 2 == 1 {
                    ui.end_row();
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.color_edit_button_srgba(&mut self.fg).on_hover_text("Foreground color");
            ui.color_edit_button_srgba(&mut self.bg).on_hover_text("Background color");
        });
        ui.add(egui::DragValue::new(&mut self.width).range(1.0..=40.0).prefix("W ")).on_hover_text("Tool width");
        ui.add(egui::DragValue::new(&mut self.eraser_width).range(2.0..=80.0).prefix("E ")).on_hover_text("Eraser width");
    }

    fn tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            let mut close = None;
            for (i, d) in self.docs.iter().enumerate() {
                let label = if d.modified { format!("{} *", d.name) } else { d.name.clone() };
                if ui.selectable_label(i == self.current, label).clicked() {
                    self.current = i;
                    self.selection = None;
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
                self.selection = None;
                self.tex_dirty = true;
            }
            if self.docs.is_empty() {
                ui.weak("No images");
            }
        });
    }

    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.status);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(d) = self.docs.get(self.current) {
                    let (w, h) = d.image().dimensions();
                    ui.label(format!("{w} x {h}"));
                    ui.separator();
                }
                if let Some((x, y)) = self.hover_px {
                    ui.label(format!("{x}, {y}"));
                    ui.separator();
                }
                if let Some(s) = self.selection {
                    ui.label(format!("Sel {}x{}", s.w, s.h));
                }
            });
        });
    }

    fn canvas(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        if !self.has_doc() {
            ui.centered_and_justified(|ui| {
                ui.weak("Capture something, or File > Open.");
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
        let avail = ui.available_size();
        let zoom = match self.zoom {
            Zoom::Percent(p) => p / 100.0,
            Zoom::Auto => ((avail.x * ppp) / iw as f32).min((avail.y * ppp) / ih as f32).clamp(0.05, 16.0),
        };
        let size = egui::vec2(iw as f32 * zoom / ppp, ih as f32 * zoom / ppp);
        let view_only = self.tool == Tool::ViewOnly;

        egui::ScrollArea::both().drag_to_scroll(view_only).show(ui, |ui| {
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click_and_drag());
            ui.painter().image(tex.id(), rect, egui::Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)), Color32::WHITE);
            let to_img = |p: Pos2| -> Pt { ((p.x - rect.min.x) * ppp / zoom, (p.y - rect.min.y) * ppp / zoom) };
            let to_ui = |p: Pt| Pos2::new(rect.min.x + p.0 * zoom / ppp, rect.min.y + p.1 * zoom / ppp);
            self.hover_px = resp.hover_pos().map(|p| {
                let q = to_img(p);
                (q.0 as i32, q.1 as i32)
            });
            if !view_only && self.tool != Tool::Magnify {
                ctx.set_cursor_icon(egui::CursorIcon::Crosshair);
            }

            // Selection overlay.
            if let Some(s) = self.selection {
                let r = egui::Rect::from_min_max(to_ui((s.x as f32, s.y as f32)), to_ui(((s.x + s.w) as f32, (s.y + s.h) as f32)));
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
                ui.painter().rect_stroke(r, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Middle);
            }

            match self.tool {
                Tool::ViewOnly => {}
                Tool::Magnify => {
                    let cur = zoom * 100.0;
                    if resp.clicked() {
                        self.zoom = Zoom::Percent((cur * 1.5).min(1600.0).round());
                    } else if resp.secondary_clicked() {
                        self.zoom = Zoom::Percent((cur / 1.5).max(5.0).round());
                    }
                }
                Tool::FloodFill => {
                    if resp.clicked() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            let (x, y) = to_img(p);
                            let c = self.fg_rgba();
                            if x >= 0.0 && y >= 0.0 {
                                self.docs[self.current].transform(|mut i| {
                                    editor::flood_fill(&mut i, x as u32, y as u32, c);
                                    i
                                });
                                self.tex_dirty = true;
                            }
                        }
                    }
                }
                Tool::Text => {
                    if resp.clicked() {
                        if let Some(p) = resp.interact_pointer_pos() {
                            self.pending_text = Some((to_img(p), String::new()));
                        }
                    }
                }
                _ => self.drag_tool(ui, &resp, &to_img, &to_ui, ppp, zoom),
            }
        });

        let mut commit = None;
        if let Some((pos, text)) = &mut self.pending_text {
            let mut open = true;
            egui::Window::new("Add Text").open(&mut open).collapsible(false).show(ctx, |ui| {
                let r = ui.text_edit_singleline(text);
                r.request_focus();
                if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
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
                let c = self.fg_rgba();
                let w = (self.width / 3.0).max(2.0);
                self.docs[self.current].add_shape(Shape { kind: Kind::Text(pos, text), color: c, width: w });
                self.tex_dirty = true;
            }
        }
    }

    fn drag_tool(&mut self, ui: &mut egui::Ui, resp: &egui::Response, to_img: &dyn Fn(Pos2) -> Pt, to_ui: &dyn Fn(Pt) -> Pos2, ppp: f32, zoom: f32) {
        let tool = self.tool;
        let freeform = matches!(tool, Tool::Freehand | Tool::Eraser | Tool::Airbrush);
        if resp.drag_started_by(egui::PointerButton::Primary) {
            if let Some(p) = resp.interact_pointer_pos() {
                self.drag = Some(vec![to_img(p)]);
            }
        }
        if resp.dragged_by(egui::PointerButton::Primary) {
            let shift = ui.input(|i| i.modifiers.shift);
            if let (Some(path), Some(p)) = (self.drag.as_mut(), resp.interact_pointer_pos()) {
                let mut q = to_img(p);
                if freeform {
                    path.push(q);
                } else {
                    if shift && matches!(tool, Tool::Ellipse | Tool::FilledEllipse | Tool::Rect | Tool::FilledRect | Tool::RoundRect | Tool::FilledRoundRect) {
                        let (dx, dy) = (q.0 - path[0].0, q.1 - path[0].1);
                        let m = dx.abs().max(dy.abs());
                        q = (path[0].0 + m * dx.signum(), path[0].1 + m * dy.signum());
                    }
                    path.truncate(1);
                    path.push(q);
                }
            }
        }
        // Live preview.
        if let Some(path) = &self.drag {
            let stroke = Stroke::new((if tool == Tool::Eraser { self.eraser_width } else { self.width }) * zoom / ppp, if tool == Tool::Eraser { self.bg } else { self.fg });
            let pts: Vec<Pos2> = path.iter().map(|p| to_ui(*p)).collect();
            let painter = ui.painter();
            match tool {
                Tool::Freehand | Tool::Eraser | Tool::Airbrush => {
                    painter.add(egui::Shape::line(pts, stroke));
                }
                _ if pts.len() == 2 => {
                    let r = egui::Rect::from_two_pos(pts[0], pts[1]);
                    match tool {
                        Tool::Line | Tool::Arrow => {
                            painter.line_segment([pts[0], pts[1]], stroke);
                        }
                        Tool::Rect => {
                            painter.rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle);
                        }
                        Tool::RoundRect => {
                            painter.rect_stroke(r, r.width().min(r.height()) / 5.0, stroke, egui::StrokeKind::Middle);
                        }
                        Tool::FilledRect => {
                            painter.rect_filled(r, 0.0, self.fg);
                        }
                        Tool::FilledRoundRect => {
                            painter.rect_filled(r, r.width().min(r.height()) / 5.0, self.fg);
                        }
                        Tool::Ellipse => {
                            painter.add(egui::epaint::EllipseShape::stroke(r.center(), r.size() / 2.0, stroke));
                        }
                        Tool::FilledEllipse => {
                            painter.add(egui::epaint::EllipseShape::filled(r.center(), r.size() / 2.0, self.fg));
                        }
                        Tool::Select => {
                            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::WHITE), egui::StrokeKind::Outside);
                            painter.rect_stroke(r, 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Middle);
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        if resp.drag_stopped() {
            let Some(path) = self.drag.take() else { return };
            let (c, w) = (self.fg_rgba(), self.width);
            match (tool, path.as_slice()) {
                (Tool::Select, [a, b]) => {
                    let (x0, y0, x1, y1) = (a.0.min(b.0), a.1.min(b.1), a.0.max(b.0), a.1.max(b.1));
                    self.selection = Some(PxRect { x: x0 as i32, y: y0 as i32, w: (x1 - x0) as i32, h: (y1 - y0) as i32 });
                }
                (Tool::Eraser, p) => {
                    let (bg, ew) = (self.bg_rgba(), self.eraser_width);
                    let p = p.to_vec();
                    self.docs[self.current].transform(|mut i| {
                        editor::erase_stroke(&mut i, &p, ew, bg);
                        i
                    });
                }
                (Tool::Airbrush, p) => {
                    let p = p.to_vec();
                    let mut seed = self.rng;
                    self.docs[self.current].transform(|mut i| {
                        editor::airbrush(&mut i, &p, w, c, &mut seed);
                        i
                    });
                    self.rng = seed;
                }
                (t, p) => {
                    let kind = match (t, p) {
                        (Tool::Freehand, p) if !p.is_empty() => Some(Kind::Pencil(p.to_vec())),
                        (Tool::Line, [a, b]) => Some(Kind::Line(*a, *b)),
                        (Tool::Arrow, [a, b]) => Some(Kind::Arrow(*a, *b)),
                        (Tool::Rect, [a, b]) => Some(Kind::Rect(*a, *b)),
                        (Tool::RoundRect, [a, b]) => Some(Kind::RoundRect(*a, *b)),
                        (Tool::Ellipse, [a, b]) => Some(Kind::Ellipse(*a, *b)),
                        (Tool::FilledRect, [a, b]) => Some(Kind::FilledRect(*a, *b)),
                        (Tool::FilledRoundRect, [a, b]) => Some(Kind::FilledRoundRect(*a, *b)),
                        (Tool::FilledEllipse, [a, b]) => Some(Kind::FilledEllipse(*a, *b)),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        self.docs[self.current].add_shape(Shape { kind, color: c, width: w });
                    }
                }
            }
            self.tex_dirty = true;
        }
    }
}
