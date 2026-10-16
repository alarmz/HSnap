//! Auto-scroll capture: scroll a window under the pointer with the mouse wheel and
//! stitch the viewport frames (vertical only, as in the original).

use crate::capture::{self, Rect};
use crate::stitch::{Added, Stitcher};
use image::RgbaImage;
use std::time::Duration;
use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::ClientToScreen;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

const MAX_STEPS: usize = 400;

/// Viewport to scroll: the client area of the control under `pt`, falling back to the
/// top-level window when that control is tiny (e.g. a scroll bar).
fn viewport_at(pt: (i32, i32)) -> Option<Rect> {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x: pt.0, y: pt.1 });
        if hwnd.0.is_null() {
            return None;
        }
        let client = |h| -> Option<Rect> {
            let mut rc = RECT::default();
            let mut o = POINT::default();
            GetClientRect(h, &mut rc).ok()?;
            if !ClientToScreen(h, &mut o).as_bool() {
                return None;
            }
            Some(Rect { x: o.x, y: o.y, w: rc.right - rc.left, h: rc.bottom - rc.top })
        };
        let r = client(hwnd)?;
        if r.w >= 100 && r.h >= 100 {
            return Some(r);
        }
        client(GetAncestor(hwnd, GA_ROOT))
    }
}

fn wheel(notches: i32) {
    let mi = MOUSEINPUT { dx: 0, dy: 0, mouseData: (-120 * notches) as u32, dwFlags: MOUSEEVENTF_WHEEL, time: 0, dwExtraInfo: 0 };
    let input = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi } };
    unsafe {
        SendInput(&[input], size_of::<INPUT>() as i32);
    }
}

fn esc_down() -> bool {
    unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) as u16 & 0x8000 != 0 }
}

/// Scroll the window at `pt` from its current position to the bottom, capturing as it goes.
/// `refresh_ms` is the pause after each scroll step (the "auto-scroll refresh time").
pub fn capture_scrolling(pt: (i32, i32), refresh_ms: u64, max_height: u32) -> Result<RgbaImage, String> {
    let view = viewport_at(pt).ok_or("no window under the pointer")?;
    let mut saved = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut saved);
    }
    let result = run(view, refresh_ms, max_height);
    unsafe {
        let _ = SetCursorPos(saved.x, saved.y);
    }
    result
}

fn run(view: Rect, refresh_ms: u64, max_height: u32) -> Result<RgbaImage, String> {
    let pause = Duration::from_millis(refresh_ms.max(50));
    // The wheel acts on the window under the pointer, so park it in the middle of the view.
    unsafe {
        let _ = SetCursorPos(view.x + view.w / 2, view.y + view.h / 2);
    }
    std::thread::sleep(pause);
    let mut st = Stitcher::new(capture::capture_rect(view, false)?);
    let mut notches = 5;
    for _ in 0..MAX_STEPS {
        if esc_down() || st.height() >= max_height {
            break;
        }
        wheel(notches);
        std::thread::sleep(pause);
        match st.add(&capture::capture_rect(view, false)?) {
            Added::Unchanged => break, // reached the end
            Added::Appended(_) => {}
            Added::NoOverlap => {
                // Scrolled too far (or the page is animating): undo the step and go slower.
                wheel(-notches);
                std::thread::sleep(pause);
                if notches == 1 {
                    break;
                }
                notches = (notches / 2).max(1);
            }
        }
    }
    Ok(st.finish())
}
