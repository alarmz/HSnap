//! Screen capture primitives built on plain GDI.

use image::RgbaImage;
use std::ffi::c_void;
use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, TRUE};
use windows::core::BOOL;
use windows::Win32::Graphics::Dwm::{DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmGetWindowAttribute};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && py >= self.y && px < self.x + self.w && py < self.y + self.h
    }
    pub fn from_corners(a: (i32, i32), b: (i32, i32)) -> Rect {
        let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
        let (y0, y1) = (a.1.min(b.1), a.1.max(b.1));
        Rect { x: x0, y: y0, w: x1 - x0, h: y1 - y0 }
    }
}

pub fn make_dpi_aware() {
    unsafe {
        let _ = SetProcessDPIAware();
    }
}

pub fn virtual_screen() -> Rect {
    unsafe {
        Rect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

pub fn cursor_pos() -> (i32, i32) {
    let mut p = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut p);
    }
    (p.x, p.y)
}

/// Monitor that currently contains the mouse pointer.
pub fn monitor_under_cursor() -> Rect {
    let (cx, cy) = cursor_pos();
    unsafe {
        let mon = MonitorFromPoint(POINT { x: cx, y: cy }, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if GetMonitorInfoW(mon, &mut mi).as_bool() {
            let r = mi.rcMonitor;
            Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
        } else {
            virtual_screen()
        }
    }
}

/// Visible, non-minimised top-level windows in z-order (front first).
pub fn visible_windows() -> Vec<Rect> {
    unsafe extern "system" fn cb(hwnd: HWND, lp: LPARAM) -> BOOL {
        let out = unsafe { &mut *(lp.0 as *mut Vec<Rect>) };
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                return TRUE;
            }
            let mut cloaked = 0u32;
            let _ = DwmGetWindowAttribute(
                hwnd,
                DWMWA_CLOAKED,
                &mut cloaked as *mut _ as *mut c_void,
                size_of::<u32>() as u32,
            );
            if cloaked != 0 {
                return TRUE;
            }
        }
        if let Some(r) = window_rect(hwnd) {
            if r.w > 0 && r.h > 0 {
                out.push(r);
            }
        }
        TRUE
    }
    let mut v: Vec<Rect> = Vec::new();
    unsafe {
        let _ = EnumWindows(Some(cb), LPARAM(&mut v as *mut _ as isize));
    }
    v
}

fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe {
        // Extended frame bounds exclude the invisible resize border on Vista+.
        let ok = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut _ as *mut c_void,
            size_of::<RECT>() as u32,
        )
        .is_ok();
        if !ok && GetWindowRect(hwnd, &mut r).is_err() {
            return None;
        }
    }
    Some(Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top })
}

pub fn active_window() -> Option<Rect> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() { None } else { window_rect(hwnd) }
}

/// Copy a screen rectangle (virtual-screen coordinates) into an RGBA image.
pub fn capture_rect(r: Rect, include_cursor: bool) -> Result<RgbaImage, String> {
    if r.w <= 0 || r.h <= 0 {
        return Err("empty capture rectangle".into());
    }
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, r.w, r.h);
        let old = SelectObject(mem, bmp.into());

        // CAPTUREBLT includes layered windows.
        let blt = BitBlt(mem, 0, 0, r.w, r.h, Some(screen), r.x, r.y, SRCCOPY | CAPTUREBLT);

        if include_cursor && blt.is_ok() {
            draw_cursor(mem, r);
        }

        let mut bi = BITMAPINFO::default();
        bi.bmiHeader = BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: r.w,
            biHeight: -r.h, // top-down
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };
        let mut buf = vec![0u8; (r.w * r.h * 4) as usize];
        let lines = GetDIBits(mem, bmp, 0, r.h as u32, Some(buf.as_mut_ptr() as *mut c_void), &mut bi, DIB_RGB_COLORS);

        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);

        blt.map_err(|e| format!("BitBlt failed: {e}"))?;
        if lines == 0 {
            return Err("GetDIBits failed".into());
        }
        for px in buf.chunks_exact_mut(4) {
            px.swap(0, 2); // BGRA -> RGBA
            px[3] = 255;
        }
        RgbaImage::from_raw(r.w as u32, r.h as u32, buf).ok_or_else(|| "bad buffer size".into())
    }
}

unsafe fn draw_cursor(hdc: HDC, r: Rect) {
    unsafe {
        let mut ci = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
        if GetCursorInfo(&mut ci).is_err() || ci.flags.0 & CURSOR_SHOWING.0 == 0 {
            return;
        }
        let mut ii = ICONINFO::default();
        let (mut hx, mut hy) = (0, 0);
        if GetIconInfo(ci.hCursor.into(), &mut ii).is_ok() {
            hx = ii.xHotspot as i32;
            hy = ii.yHotspot as i32;
            if !ii.hbmMask.is_invalid() {
                let _ = DeleteObject(ii.hbmMask.into());
            }
            if !ii.hbmColor.is_invalid() {
                let _ = DeleteObject(ii.hbmColor.into());
            }
        }
        let _ = DrawIconEx(hdc, ci.ptScreenPos.x - r.x - hx, ci.ptScreenPos.y - r.y - hy, ci.hCursor.into(), 0, 0, 0, None, DI_NORMAL);
    }
}

pub fn capture_full_screen(include_cursor: bool) -> Result<RgbaImage, String> {
    capture_rect(monitor_under_cursor(), include_cursor)
}

pub fn capture_virtual_desktop(include_cursor: bool) -> Result<RgbaImage, String> {
    capture_rect(virtual_screen(), include_cursor)
}

pub fn capture_active_window(include_cursor: bool) -> Result<RgbaImage, String> {
    let r = active_window().ok_or("no active window")?;
    capture_rect(r, include_cursor)
}

/// Image of the pointer only, drawn on `bg`.
pub fn capture_cursor_only(bg: [u8; 4]) -> Result<RgbaImage, String> {
    let (cx, cy) = cursor_pos();
    let r = Rect { x: cx - 32, y: cy - 32, w: 64, h: 64 };
    let mut img = RgbaImage::from_pixel(64, 64, image::Rgba(bg));
    let shot = capture_rect(r, true)?;
    let plain = capture_rect(r, false)?;
    // Keep only pixels the cursor changed.
    for (x, y, p) in shot.enumerate_pixels() {
        if plain.get_pixel(x, y) != p {
            img.put_pixel(x, y, *p);
        }
    }
    Ok(img)
}

/// Client area of the foreground window, i.e. the window without its frame.
pub fn capture_active_client(include_cursor: bool) -> Result<RgbaImage, String> {
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() {
        return Err("no active window".into());
    }
    let mut rc = RECT::default();
    let mut origin = POINT::default();
    unsafe {
        GetClientRect(hwnd, &mut rc).map_err(|e| e.to_string())?;
        if !ClientToScreen(hwnd, &mut origin).as_bool() {
            return Err("ClientToScreen failed".into());
        }
    }
    capture_rect(Rect { x: origin.x, y: origin.y, w: rc.right - rc.left, h: rc.bottom - rc.top }, include_cursor)
}
