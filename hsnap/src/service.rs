//! Settings and the capture worker. Everything here runs off the UI thread so
//! hot keys keep working while the main window is minimised.

use crate::capture::{self, Rect};
use crate::hotkeys::HotkeyAction;
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
    Region,
    Window,
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
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
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
        let hide = s.hide_window;
        let was_visible = hide && own_window().is_some_and(|h| unsafe { !IsIconic(h).as_bool() });
        if was_visible {
            show_own(SW_MINIMIZE);
        }
        if was_visible || s.delay_ms > 0 {
            // Give the minimise animation time to finish, then honour the delay.
            std::thread::sleep(Duration::from_millis(s.delay_ms + if was_visible { 400 } else { 0 }));
        }
        let res = match what {
            Last::Mode(m) => capture_mode(m, &s),
            Last::Rect(r) => capture::capture_rect(r, s.include_cursor).map(|i| (i, Last::Rect(r), "Repeat")),
        };
        if was_visible {
            show_own(SW_RESTORE);
        }
        match res {
            Ok((img, last, label)) => {
                if remember {
                    *self.last.lock().unwrap() = Some((last, label));
                }
                let _ = self.tx.send(Captured { img, label });
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

fn show_own(cmd: SHOW_WINDOW_CMD) {
    if let Some(h) = own_window() {
        unsafe {
            let _ = ShowWindow(h, cmd);
            if cmd == SW_RESTORE {
                let _ = SetForegroundWindow(h);
            }
        }
    }
}

fn capture_mode(m: CaptureMode, s: &Settings) -> Result<(RgbaImage, Last, &'static str), String> {
    let cur = s.include_cursor;
    Ok(match m {
        CaptureMode::Full => {
            let r = capture::monitor_under_cursor();
            (capture::capture_rect(r, cur)?, Last::Rect(r), "Full screen")
        }
        CaptureMode::Virtual => (capture::capture_virtual_desktop(cur)?, Last::Mode(m), "Virtual desktop"),
        CaptureMode::Active => (capture::capture_active_window(cur)?, Last::Mode(m), "Active window"),
        CaptureMode::Cursor => (capture::capture_cursor_only(s.background)?, Last::Mode(m), "Cursor"),
        CaptureMode::Region | CaptureMode::Window => {
            let (arg, label) = if m == CaptureMode::Region { ("region", "Region") } else { ("window", "Window") };
            let tmp = std::env::temp_dir().join(format!("hsnap-pick-{}.png", std::process::id()));
            let status = std::process::Command::new(std::env::current_exe().map_err(|e| e.to_string())?)
                .args(["--pick", arg])
                .arg(&tmp)
                .arg(if cur { "1" } else { "0" })
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
            (img, rect.map(Last::Rect).unwrap_or(Last::Mode(m)), label)
        }
    })
}
