//! GDI output for printing: printer device contexts, header/footer text, image slices.

use crate::print::{Justify, PageSetup, expand_text, landscape_for, layout, prepare_image};
use image::RgbaImage;
use std::ffi::c_void;
use std::path::PathBuf;
use windows::Win32::Foundation::{HWND, SIZE};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::Storage::Xps::{DOCINFOW, EndDoc, EndPage, StartDocW, StartPage};
use windows::Win32::UI::Controls::Dialogs::*;
use windows::core::PCWSTR;

pub enum Target {
    /// Standard print dialog.
    Dialog(Option<HWND>),
    /// System default printer and settings, no dialog.
    DefaultPrinter,
    /// A named printer, optionally printing to a file (for example "Microsoft Print to PDF").
    Named { printer: String, output: Option<PathBuf> },
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn dc_for(target: &Target, landscape: bool) -> Result<HDC, String> {
    unsafe {
        match target {
            Target::Named { printer, .. } => {
                let name = wide(printer);
                let dc = CreateDCW(windows::core::w!("WINSPOOL"), PCWSTR(name.as_ptr()), PCWSTR::null(), None);
                if dc.is_invalid() { Err(format!("cannot open printer {printer}")) } else { Ok(dc) }
            }
            Target::Dialog(_) | Target::DefaultPrinter => {
                let mut pd = PRINTDLGW { lStructSize: size_of::<PRINTDLGW>() as u32, ..Default::default() };
                pd.Flags = PD_RETURNDC | PD_NOSELECTION | PD_NOPAGENUMS;
                match target {
                    Target::Dialog(owner) => pd.hwndOwner = owner.unwrap_or_default(),
                    _ => pd.Flags |= PD_RETURNDEFAULT,
                }
                if !PrintDlgW(&mut pd).as_bool() {
                    return Err(if CommDlgExtendedError().0 == 0 { "cancelled".into() } else { "print dialog failed".into() });
                }
                if pd.hDC.is_invalid() {
                    return Err("no printer device context".into());
                }
                set_orientation(pd.hDC, pd.hDevMode, landscape);
                Ok(pd.hDC)
            }
        }
    }
}

/// Apply the orientation to the DC through the DEVMODE the dialog returned.
unsafe fn set_orientation(dc: HDC, hdevmode: windows::Win32::Foundation::HGLOBAL, landscape: bool) {
    use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
    unsafe {
        if hdevmode.0.is_null() {
            return;
        }
        let dm = GlobalLock(hdevmode) as *mut DEVMODEW;
        if dm.is_null() {
            return;
        }
        (*dm).dmFields |= DM_ORIENTATION;
        (*dm).Anonymous1.Anonymous1.dmOrientation = if landscape { DMORIENT_LANDSCAPE as i16 } else { DMORIENT_PORTRAIT as i16 };
        ResetDCW(dc, dm);
        let _ = GlobalUnlock(hdevmode);
    }
}

/// Print `img` using `setup`. Returns "cancelled" when the user dismisses the dialog.
pub fn print_image(img: &RgbaImage, setup: &PageSetup, doc_name: &str, img_dpi: (f32, f32), target: Target) -> Result<(), String> {
    let img = prepare_image(img, setup);
    let dc = dc_for(&target, landscape_for(setup, img.width(), img.height()))?;
    let result = unsafe { print_on(dc, &img, setup, doc_name, img_dpi, &target) };
    unsafe {
        let _ = DeleteDC(dc);
    }
    result
}

unsafe fn print_on(dc: HDC, img: &RgbaImage, setup: &PageSetup, doc_name: &str, img_dpi: (f32, f32), target: &Target) -> Result<(), String> {
    unsafe {
        let dpi_x = GetDeviceCaps(Some(dc), LOGPIXELSX) as f32;
        let dpi_y = GetDeviceCaps(Some(dc), LOGPIXELSY) as f32;
        let (hres, vres) = (GetDeviceCaps(Some(dc), HORZRES) as f32, GetDeviceCaps(Some(dc), VERTRES) as f32);
        let (pw, ph) = (GetDeviceCaps(Some(dc), PHYSICALWIDTH) as f32, GetDeviceCaps(Some(dc), PHYSICALHEIGHT) as f32);
        let (ox, oy) = (GetDeviceCaps(Some(dc), PHYSICALOFFSETX) as f32, GetDeviceCaps(Some(dc), PHYSICALOFFSETY) as f32);
        // Margins are measured from the paper edge; coordinates here start at the printable area.
        let m = setup.margins;
        let margins = [
            (m[0] * dpi_x - ox).max(0.0),
            (m[1] * dpi_y - oy).max(0.0),
            (m[2] * dpi_x - (pw - ox - hres)).max(0.0),
            (m[3] * dpi_y - (ph - oy - vres)).max(0.0),
        ];
        let line_h = setup.font_pt * dpi_y / 72.0;
        let top = if setup.header.is_empty() { 0.0 } else { line_h * 1.6 };
        let bottom = if setup.footer.is_empty() { 0.0 } else { line_h * 1.6 };
        let pages = layout(img.width(), img.height(), (hres, vres), margins, top, bottom, setup, dpi_x / img_dpi.0.max(1.0));

        let name = wide(doc_name);
        let out = match target {
            Target::Named { output: Some(p), .. } => Some(wide(&p.to_string_lossy())),
            _ => None,
        };
        let di = DOCINFOW {
            cbSize: size_of::<DOCINFOW>() as i32,
            lpszDocName: PCWSTR(name.as_ptr()),
            lpszOutput: out.as_ref().map_or(PCWSTR::null(), |o| PCWSTR(o.as_ptr())),
            ..Default::default()
        };
        if StartDocW(dc, &di) <= 0 {
            return Err("StartDoc failed".into());
        }
        SetStretchBltMode(dc, HALFTONE);
        let font = CreateFontW(
            -(line_h.round() as i32),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            DEFAULT_QUALITY,
            0,
            windows::core::w!("Segoe UI"),
        );
        let old_font = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);

        let n = pages.len();
        for (i, pg) in pages.iter().enumerate() {
            if StartPage(dc) <= 0 {
                let _ = EndDoc(dc);
                return Err("StartPage failed".into());
            }
            // Header and footer.
            for (text, just, y) in [(&setup.header, setup.header_justify, margins[1]), (&setup.footer, setup.footer_justify, vres - margins[3] - line_h * 1.2)] {
                if text.is_empty() {
                    continue;
                }
                let t: Vec<u16> = expand_text(text, i + 1, n, doc_name).encode_utf16().collect();
                let mut size = SIZE::default();
                let _ = GetTextExtentPoint32W(dc, &t, &mut size);
                let usable = hres - margins[0] - margins[2];
                let x = match just {
                    Justify::Left => margins[0],
                    Justify::Center => margins[0] + (usable - size.cx as f32) / 2.0,
                    Justify::Right => margins[0] + usable - size.cx as f32,
                };
                let _ = TextOutW(dc, x as i32, y as i32, &t);
            }
            // Image slice as a top-down 32-bit DIB.
            let stride = img.width() as usize * 4;
            let slice = &img.as_raw()[pg.src_y as usize * stride..(pg.src_y + pg.src_h) as usize * stride];
            let mut buf = Vec::with_capacity(slice.len());
            for px in slice.chunks_exact(4) {
                buf.extend_from_slice(&[px[2], px[1], px[0], 0]);
            }
            let mut bi = BITMAPINFO::default();
            bi.bmiHeader = BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: img.width() as i32,
                biHeight: -(pg.src_h as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            };
            StretchDIBits(dc, pg.x as i32, pg.y as i32, pg.w as i32, pg.h as i32, 0, 0, img.width() as i32, pg.src_h as i32, Some(buf.as_ptr() as *const c_void), &bi, DIB_RGB_COLORS, SRCCOPY);
            if setup.draw_frame {
                let old = SelectObject(dc, GetStockObject(NULL_BRUSH));
                let _ = Rectangle(dc, pg.x as i32 - 1, pg.y as i32 - 1, (pg.x + pg.w) as i32 + 1, (pg.y + pg.h) as i32 + 1);
                SelectObject(dc, old);
            }
            if EndPage(dc) <= 0 {
                let _ = EndDoc(dc);
                return Err("EndPage failed".into());
            }
        }
        SelectObject(dc, old_font);
        let _ = DeleteObject(font.into());
        if EndDoc(dc) <= 0 {
            return Err("EndDoc failed".into());
        }
        Ok(())
    }
}
