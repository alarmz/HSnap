//! Settings and the capture worker. Everything here runs off the UI thread so
//! hot keys keep working while the main window is minimised.

use crate::capture::{self, Rect};
use crate::compose::Compose;
use crate::picker;
use crate::scroll;
use crate::hotkeys::{self, Binding, HotkeyAction};
use eframe::egui;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

pub const WINDOW_TITLE: &str = "HSnap";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureMode {
    Full,
    Virtual,
    Active,
    ActiveNoFrame,
    Region,
    Window,
    FreeHand,
    Multi,
    Pan,
    Scroll,
    TextRegion,
    TextObject,
    TextWindowScroll,
    TextRegionScroll,
    Cursor,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub delay_ms: u64,
    pub include_cursor: bool,
    pub hide_window: bool,
    pub background: [u8; 4],
    pub auto_copy: bool,
    pub quicksave: bool,
    pub quicksave_dir: String,
    pub quicksave_template: String,
    pub quicksave_start: i64,
    pub quicksave_stop: String,
    pub quicksave_step: i64,
    pub quicksave_loop: bool,
    pub repeat_secs: f32,
    pub hotkeys_enabled: bool,
    pub hotkeys: Vec<(HotkeyAction, Binding)>,
    pub restore_window: bool,
    pub play_sound: bool,
    pub bg_transparent: bool,
    pub mark_modified: bool,
    pub only_if_changed: bool,
    pub view_edit: ViewEdit,
    pub show_region_help: bool,
    pub multi_compose: Compose,
    pub multi_margin: u32,
    pub multi_start_window: bool,
    pub scroll_refresh_ms: u64,
    pub scroll_max_height: u32,
    pub autostart: bool,
    pub autostart_all_users: bool,
    pub start_minimized: bool,
    pub show_tray: bool,
    pub keep_in_tray: bool,
    pub recent_count: u32,
    pub recent: Vec<String>,
    pub page_setup: crate::print::PageSetup,
    pub auto_print: bool,
    pub auto_print_prompt: bool,
    pub stamps_dir: String,
    pub text_mode: crate::textsnap::TextMode,
    pub text_show_window: bool,
    pub text_append: bool,
    pub text_delay_ms: u64,
    pub ocr_lang: String,
    pub shadow: crate::filters::ShadowParams,
    pub frame: crate::filters::FrameParams,
    pub subst: crate::filters::SubstTable,
}

/// What happens with each new capture (View & Edit tab).
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default, Debug)]
pub enum ViewEdit {
    #[default]
    NewWindow,
    Replace,
    DoNotChange,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            delay_ms: 0,
            include_cursor: false,
            hide_window: true,
            background: [255, 255, 255, 255],
            auto_copy: false,
            quicksave: false,
            quicksave_dir: dirs_pictures(),
            quicksave_template: "Capture001.png".into(),
            quicksave_start: 1,
            quicksave_stop: String::new(),
            quicksave_step: 1,
            quicksave_loop: false,
            repeat_secs: 20.0,
            hotkeys_enabled: true,
            hotkeys: hotkeys::defaults(),
            restore_window: true,
            play_sound: false,
            bg_transparent: false,
            mark_modified: false,
            only_if_changed: false,
            view_edit: ViewEdit::NewWindow,
            show_region_help: true,
            multi_compose: Compose::Preserve,
            multi_margin: 0,
            multi_start_window: false,
            scroll_refresh_ms: 300,
            scroll_max_height: 30_000,
            autostart: false,
            autostart_all_users: false,
            start_minimized: false,
            show_tray: false,
            keep_in_tray: false,
            recent_count: 4,
            recent: vec![],
            page_setup: Default::default(),
            auto_print: false,
            auto_print_prompt: true,
            stamps_dir: crate::stamps::Store::default_dir().to_string_lossy().into_owned(),
            text_mode: Default::default(),
            text_show_window: true,
            text_append: true,
            text_delay_ms: 0,
            ocr_lang: String::new(),
            shadow: Default::default(),
            frame: Default::default(),
            subst: Default::default(),
        }
    }
}

fn dirs_pictures() -> String {
    std::env::var("USERPROFILE").map(|p| format!(r"{p}\Pictures")).unwrap_or_else(|_| ".".into())
}

impl Settings {
    fn path() -> Option<PathBuf> {
        std::env::var("APPDATA").ok().map(|a| PathBuf::from(a).join("hsnap").join("settings.json"))
    }
    pub fn load() -> Self {
        let mut s: Settings = Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        s.normalize_hotkeys();
        s
    }

    /// Settings saved by an older build may lack newer actions or clash with new defaults.
    fn normalize_hotkeys(&mut self) {
        let defs = hotkeys::defaults();
        let clash = self.hotkeys.iter().enumerate().any(|(i, (_, b))| self.hotkeys.iter().skip(i + 1).any(|(_, c)| c == b));
        if clash {
            self.hotkeys = defs;
            return;
        }
        for (a, b) in defs {
            if !self.hotkeys.iter().any(|(x, _)| *x == a) && !self.hotkeys.iter().any(|(_, c)| *c == b) {
                self.hotkeys.push((a, b));
            }
        }
    }
    pub fn save(&self) {
        if let Some(p) = Self::path() {
            let _ = std::fs::create_dir_all(p.parent().unwrap());
            let _ = std::fs::write(p, serde_json::to_string_pretty(self).unwrap_or_default());
        }
    }
}

pub struct Captured {
    pub img: RgbaImage,
    pub label: &'static str,
    /// Set for TextSnap captures: the recognised text (the image is then a 1x1 placeholder).
    pub text: Option<String>,
}

#[derive(Clone, Copy)]
enum Last {
    Mode(CaptureMode),
    Rect(Rect),
}

#[derive(Clone)]
pub struct Service {
    tx: Sender<Captured>,
    ctx: egui::Context,
    pub settings: Arc<Mutex<Settings>>,
    last: Arc<Mutex<Option<(Last, &'static str)>>>,
    auto_running: Arc<AtomicBool>,
}

impl Service {
    pub fn new(tx: Sender<Captured>, ctx: egui::Context, settings: Settings) -> Self {
        Self { tx, ctx, settings: Arc::new(Mutex::new(settings)), last: Default::default(), auto_running: Default::default() }
    }

    pub fn is_auto_running(&self) -> bool {
        self.auto_running.load(Ordering::Relaxed)
    }

    pub fn handle_hotkey(&self, a: HotkeyAction) {
        if !self.settings.lock().unwrap().hotkeys_enabled {
            return;
        }
        match a {
            HotkeyAction::Region => self.capture(CaptureMode::Region),
            HotkeyAction::FullScreen => self.capture(CaptureMode::Full),
            HotkeyAction::Window => self.capture(CaptureMode::Window),
            HotkeyAction::ActiveWindow => self.capture(CaptureMode::Active),
            HotkeyAction::Cursor => self.capture(CaptureMode::Cursor),
            HotkeyAction::MultiRegion => self.capture(CaptureMode::Multi),
            HotkeyAction::ScrollPage => self.capture(CaptureMode::Scroll),
            HotkeyAction::TextRegion => self.capture(CaptureMode::TextRegion),
            HotkeyAction::TextObject => self.capture(CaptureMode::TextObject),
            HotkeyAction::RepeatLast => self.repeat_last(),
            HotkeyAction::StopAuto => self.auto_running.store(false, Ordering::Relaxed),
        }
    }

    pub fn capture(&self, mode: CaptureMode) {
        let me = self.clone();
        std::thread::spawn(move || {
            me.run_once(Last::Mode(mode), true);
        });
    }

    pub fn repeat_last(&self) {
        let last = self.last.lock().unwrap().map(|l| l.0);
        if let Some(l) = last {
            let me = self.clone();
            std::thread::spawn(move || {
                me.run_once(l, false);
            });
        }
    }

    /// Repeat the last capture every `secs` seconds until stopped (Shift+F11).
    pub fn start_auto(&self, secs: f32) {
        if self.last.lock().unwrap().is_none() || self.auto_running.swap(true, Ordering::Relaxed) {
            return;
        }
        let me = self.clone();
        std::thread::spawn(move || {
            while me.auto_running.load(Ordering::Relaxed) {
                let l = me.last.lock().unwrap().map(|l| l.0);
                if let Some(l) = l {
                    me.run_once(l, false);
                }
                let mut left = (secs.max(0.1) * 1000.0) as u64;
                while left > 0 && me.auto_running.load(Ordering::Relaxed) {
                    let step = left.min(100);
                    std::thread::sleep(Duration::from_millis(step));
                    left -= step;
                }
            }
        });
    }

    pub fn stop_auto(&self) {
        self.auto_running.store(false, Ordering::Relaxed);
    }

    fn run_once(&self, what: Last, remember: bool) {
        let s = self.settings.lock().unwrap().clone();
        // Reading the object under the pointer must not disturb it (a menu would close).
        let hide = s.hide_window && !matches!(what, Last::Mode(CaptureMode::TextObject));
        let was_visible = hide && own_window().is_some_and(|h| unsafe { !IsIconic(h).as_bool() });
        if was_visible {
            show_own(SW_MINIMIZE);
        }
        if was_visible || s.delay_ms > 0 {
            // Give the minimise animation time to finish, then honour the delay.
            std::thread::sleep(Duration::from_millis(s.delay_ms + if was_visible { 400 } else { 0 }));
        }
        if let Last::Mode(m @ (CaptureMode::TextRegion | CaptureMode::TextObject | CaptureMode::TextWindowScroll | CaptureMode::TextRegionScroll)) = what {
            let res = capture_text(m, &s);
            if was_visible && s.restore_window {
                show_own(SW_RESTORE);
            }
            match res {
                Ok(text) => {
                    if s.play_sound {
                        unsafe {
                            let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_OK);
                        }
                    }
                    let _ = self.tx.send(Captured { img: RgbaImage::new(1, 1), label: "Text", text: Some(text) });
                    self.ctx.request_repaint();
                }
                Err(e) if e == "cancelled" => {}
                Err(e) => {
                    let _ = self.tx.send(Captured { img: RgbaImage::new(1, 1), label: "Text", text: Some(format!("[TextSnap] {e}")) });
                    self.ctx.request_repaint();
                }
            }
            return;
        }
        let pan = self.last.lock().unwrap().and_then(|(l, _)| match l {
            Last::Rect(r) => Some((r.w, r.h)),
            Last::Mode(_) => None,
        });
        let res = match what {
            Last::Mode(m) => capture_mode(m, &s, pan),
            Last::Rect(r) => capture::capture_rect(r, s.include_cursor).map(|i| (i, Last::Rect(r), "Repeat")),
        };
        if was_visible && s.restore_window {
            show_own(SW_RESTORE);
        }
        if s.play_sound {
            unsafe {
                let _ = windows::Win32::System::Diagnostics::Debug::MessageBeep(MB_OK);
            }
        }
        match res {
            Ok((img, last, label)) => {
                if remember {
                    *self.last.lock().unwrap() = Some((last, label));
                }
                let _ = self.tx.send(Captured { img, label, text: None });
                self.ctx.request_repaint();
            }
            Err(e) if e == "cancelled" => {}
            Err(e) => eprintln!("capture failed: {e}"),
        }
    }
}

fn own_window() -> Option<windows::Win32::Foundation::HWND> {
    let _ = WINDOW_TITLE;
    unsafe { FindWindowW(None, w!("HSnap")).ok() }
}

pub fn show_own(cmd: SHOW_WINDOW_CMD) {
    if let Some(h) = own_window() {
        unsafe {
            let _ = ShowWindow(h, cmd);
            if cmd == SW_RESTORE {
                let _ = SetForegroundWindow(h);
            }
        }
    }
}

fn capture_mode(m: CaptureMode, s: &Settings, pan: Option<(i32, i32)>) -> Result<(RgbaImage, Last, &'static str), String> {
    let cur = s.include_cursor;
    Ok(match m {
        CaptureMode::Full => {
            let r = capture::monitor_under_cursor();
            (capture::capture_rect(r, cur)?, Last::Rect(r), "Full screen")
        }
        CaptureMode::Virtual => (capture::capture_virtual_desktop(cur)?, Last::Mode(m), "Virtual desktop"),
        CaptureMode::Active => (capture::capture_active_window(cur)?, Last::Mode(m), "Active window"),
        CaptureMode::ActiveNoFrame => (capture::capture_active_client(cur)?, Last::Mode(m), "Active window without frame"),
        CaptureMode::Cursor => (capture::capture_cursor_only(s.background)?, Last::Mode(m), "Cursor"),
        // Text modes are handled before this function (they produce text, not an image).
        CaptureMode::TextRegion | CaptureMode::TextObject | CaptureMode::TextWindowScroll | CaptureMode::TextRegionScroll => return Err("text capture has no image".into()),
        CaptureMode::Scroll => {
            let (_, click) = run_picker(picker::Mode::ScrollTarget, s, None)?;
            let pt = click.ok_or("no click position")?;
            (scroll::capture_scrolling((pt.x, pt.y), s.scroll_refresh_ms, s.scroll_max_height)?, Last::Mode(m), "Entire page")
        }
        CaptureMode::Region | CaptureMode::Window | CaptureMode::FreeHand | CaptureMode::Multi | CaptureMode::Pan => {
            let (mode, label) = match m {
                CaptureMode::Region => (picker::Mode::Region, "Region"),
                CaptureMode::Window => (picker::Mode::Window, "Window"),
                CaptureMode::FreeHand => (picker::Mode::FreeHand, "Free hand"),
                CaptureMode::Multi => (picker::Mode::Multi, "Multi-region"),
                _ => (picker::Mode::Pan, "Pan region"),
            };
            if mode == picker::Mode::Pan && pan.is_none() {
                return Err("no previous region to pan".into());
            }
            let (img, rect) = run_picker(mode, s, pan)?;
            (img, rect.map(Last::Rect).unwrap_or(Last::Mode(m)), label)
        }
    })
}

/// TextSnap: text from a region, the object under the pointer, or a scrolled window/region.
fn capture_text(m: CaptureMode, s: &Settings) -> Result<String, String> {
    let from_img = |img: &RgbaImage| crate::textsnap::from_image(img, &s.ocr_lang, s.text_mode);
    let text = match m {
        CaptureMode::TextRegion => from_img(&run_picker(picker::Mode::Region, s, None)?.0)?,
        CaptureMode::TextObject => {
            std::thread::sleep(Duration::from_millis(s.text_delay_ms));
            crate::textsnap::from_object(capture::cursor_pos(), &s.ocr_lang, s.text_mode)?
        }
        CaptureMode::TextWindowScroll => {
            let click = run_picker(picker::Mode::ScrollTarget, s, None)?.1.ok_or("no click position")?;
            from_img(&scroll::capture_scrolling((click.x, click.y), s.scroll_refresh_ms, s.scroll_max_height)?)?
        }
        _ => {
            let rect = run_picker(picker::Mode::Region, s, None)?.1.ok_or("no region")?;
            from_img(&scroll::capture_scrolling_rect(rect, s.scroll_refresh_ms, s.scroll_max_height)?)?
        }
    };
    if text.trim().is_empty() { Err("no text found".into()) } else { Ok(text) }
}

/// Runs the selection overlay in a child process; returns its image and screen rectangle.
fn run_picker(mode: picker::Mode, s: &Settings, pan: Option<(i32, i32)>) -> Result<(RgbaImage, Option<Rect>), String> {
    let mut bg = s.background;
    if s.bg_transparent {
        bg[3] = 0;
    }
    let params = picker::Params {
        mode,
        out: std::env::temp_dir().join(format!("hsnap-pick-{}.png", std::process::id())),
        include_cursor: s.include_cursor,
        bg,
        compose: s.multi_compose,
        margin: s.multi_margin,
        pan_size: pan,
        show_help: s.show_region_help,
        multi_start_window: s.multi_start_window,
    };
    let tmp = params.out.clone();
    let status = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
        .args(params.to_args())
        .status()
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(if status.code() == Some(1) { "cancelled".into() } else { "picker failed".into() });
    }
    let img = image::open(&tmp).map_err(|e| e.to_string())?.to_rgba8();
    let rect = std::fs::read_to_string(tmp.with_extension("rect")).ok().and_then(|t| {
        let v: Vec<i32> = t.split_whitespace().filter_map(|x| x.parse().ok()).collect();
        (v.len() == 4).then(|| Rect { x: v[0], y: v[1], w: v[2], h: v[3] })
    });
    let _ = std::fs::remove_file(&tmp);
    let _ = std::fs::remove_file(tmp.with_extension("rect"));
    Ok((img, rect))
}
