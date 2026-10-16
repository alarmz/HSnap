//! TextSnap: capture text from the screen, using UI Automation where the target exposes
//! its text and OCR otherwise.

#![allow(non_upper_case_globals)] // the UIA_* control-type constants keep their Windows names

use crate::capture::{self, Rect};
use crate::ocr;
use image::RgbaImage;
use serde::{Deserialize, Serialize};
use windows::Win32::Foundation::POINT;
use windows::Win32::System::Com::{CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx};
use windows::Win32::UI::Accessibility::*;
use windows::core::Interface;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum TextMode {
    #[default]
    Plain,
    /// Columns separated by tabs, ready to paste into a spreadsheet.
    TabDelimited,
}

/// Text recognised in a bitmap.
pub fn from_image(img: &RgbaImage, lang: &str, mode: TextMode) -> Result<String, String> {
    let lines = ocr::recognize_tall(img, lang)?;
    Ok(match mode {
        TextMode::Plain => ocr::plain_text(&lines),
        TextMode::TabDelimited => ocr::tab_text(&lines),
    })
}

/// What UI Automation could read at a point.
pub struct Found {
    pub text: String,
    pub rect: Option<Rect>,
}

fn is_item(t: UIA_CONTROLTYPE_ID) -> bool {
    matches!(t, UIA_MenuItemControlTypeId | UIA_ListItemControlTypeId | UIA_TreeItemControlTypeId | UIA_DataItemControlTypeId | UIA_TabItemControlTypeId)
}

fn is_row(t: UIA_CONTROLTYPE_ID) -> bool {
    matches!(t, UIA_ListItemControlTypeId | UIA_DataItemControlTypeId | UIA_TreeItemControlTypeId)
}

/// Text the element itself carries: a document's text, or an edit box's value.
fn own_text(el: &IUIAutomationElement) -> String {
    unsafe {
        if let Ok(p) = el.GetCurrentPattern(UIA_TextPatternId) {
            if let Ok(tp) = p.cast::<IUIAutomationTextPattern>() {
                if let Ok(range) = tp.DocumentRange() {
                    if let Ok(t) = range.GetText(-1) {
                        let t = t.to_string();
                        if !t.trim().is_empty() {
                            return t;
                        }
                    }
                }
            }
        }
        if let Ok(p) = el.GetCurrentPattern(UIA_ValuePatternId) {
            if let Ok(vp) = p.cast::<IUIAutomationValuePattern>() {
                if let Ok(v) = vp.CurrentValue() {
                    return v.to_string();
                }
            }
        }
    }
    String::new()
}

fn name_of(el: &IUIAutomationElement) -> String {
    unsafe { el.CurrentName().map(|n| n.to_string()).unwrap_or_default().trim().to_string() }
}

fn children(uia: &IUIAutomation, el: &IUIAutomationElement) -> Vec<IUIAutomationElement> {
    unsafe {
        let Ok(cond) = uia.CreateTrueCondition() else { return vec![] };
        let Ok(arr) = el.FindAll(TreeScope_Children, &cond) else { return vec![] };
        let n = arr.Length().unwrap_or(0).min(400);
        (0..n).filter_map(|i| arr.GetElement(i).ok()).collect()
    }
}

/// Names of the leaves below `el`, in tree order.
fn leaf_names(uia: &IUIAutomation, el: &IUIAutomationElement, depth: u32, out: &mut Vec<String>) {
    let kids = if depth == 0 { vec![] } else { children(uia, el) };
    if kids.is_empty() {
        let n = name_of(el);
        if !n.is_empty() {
            out.push(n);
        }
        return;
    }
    for k in kids {
        leaf_names(uia, &k, depth - 1, out);
    }
}

fn collect(uia: &IUIAutomation, el: &IUIAutomationElement, depth: u32, out: &mut Vec<String>) {
    let ty = unsafe { el.CurrentControlType().unwrap_or_default() };
    let own = own_text(el);
    if !own.trim().is_empty() && !is_item(ty) {
        out.push(own.trim_end().to_string());
        return;
    }
    if is_row(ty) {
        let mut cells = vec![];
        leaf_names(uia, el, 3, &mut cells);
        if cells.is_empty() {
            let n = name_of(el);
            if !n.is_empty() {
                cells.push(n);
            }
        }
        if !cells.is_empty() {
            out.push(cells.join("\t"));
        }
        return;
    }
    let kids = if depth == 0 { vec![] } else { children(uia, el) };
    if kids.is_empty() {
        let n = name_of(el);
        if !n.is_empty() {
            out.push(n);
        }
        return;
    }
    if ty == UIA_MenuItemControlTypeId {
        let n = name_of(el);
        if !n.is_empty() {
            out.push(n);
        }
    }
    for k in kids {
        collect(uia, &k, depth - 1, out);
    }
}

/// Read the text of the object at `pt`. Items (menu entries, list rows) are widened to their
/// container so the whole menu or list is captured, as the manual describes.
pub fn text_at(pt: (i32, i32)) -> Result<Found, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let mut el = uia.ElementFromPoint(POINT { x: pt.0, y: pt.1 }).map_err(|e| e.to_string())?;
        if is_item(el.CurrentControlType().unwrap_or_default()) {
            if let Ok(walker) = uia.ControlViewWalker() {
                if let Ok(parent) = walker.GetParentElement(&el) {
                    el = parent;
                }
            }
        }
        let rect = el.CurrentBoundingRectangle().ok().map(|r| Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }).filter(|r| r.w > 0 && r.h > 0);
        let mut lines = vec![];
        collect(&uia, &el, 6, &mut lines);
        lines.dedup();
        Ok(Found { text: lines.join("\n"), rect })
    }
}

/// Text of a whole window (by handle); used for testing without relying on hit-testing.
pub fn text_of_window(hwnd: windows::Win32::Foundation::HWND) -> Result<String, String> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let uia: IUIAutomation = CoCreateInstance(&CUIAutomation, None, CLSCTX_INPROC_SERVER).map_err(|e| e.to_string())?;
        let el = uia.ElementFromHandle(hwnd).map_err(|e| e.to_string())?;
        let mut lines = vec![];
        collect(&uia, &el, 6, &mut lines);
        lines.dedup();
        Ok(lines.join("
"))
    }
}

/// Text of the object under `pt`: UI Automation first, OCR of the object's area as a fallback.
pub fn from_object(pt: (i32, i32), lang: &str, mode: TextMode) -> Result<String, String> {
    let found = text_at(pt).unwrap_or(Found { text: String::new(), rect: None });
    if !found.text.trim().is_empty() {
        return Ok(found.text);
    }
    let rect = found.rect.ok_or("nothing readable under the pointer")?;
    let shot = capture::capture_rect(rect, false)?;
    let text = from_image(&shot, lang, mode)?;
    if text.trim().is_empty() { Err("no text found".into()) } else { Ok(text) }
}
