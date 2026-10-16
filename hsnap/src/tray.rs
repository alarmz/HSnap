//! System tray icon and its context menu. Handlers run on the Win32 message thread and
//! act through `Service` and plain window calls so they work while the window is hidden.

use crate::service::{CaptureMode, Service, show_own};
use eframe::egui;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};
use windows::Win32::UI::WindowsAndMessaging::{SW_RESTORE, SW_SHOW};

pub struct Tray {
    _icon: TrayIcon,
}

/// A small camera-like glyph, drawn in code.
fn icon_image() -> Icon {
    const N: u32 = 32;
    let mut rgba = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            let body = (3.0..29.0).contains(&fx) && (8.0..27.0).contains(&fy);
            let top = (10.0..22.0).contains(&fx) && (4.0..9.0).contains(&fy);
            let d = ((fx - 16.0).powi(2) + (fy - 17.0).powi(2)).sqrt();
            let px = if d < 5.0 {
                Some([240, 245, 255, 255])
            } else if d < 8.5 {
                Some([30, 40, 70, 255])
            } else if body || top {
                Some([70, 110, 200, 255])
            } else {
                None
            };
            if let Some(c) = px {
                let i = ((y * N + x) * 4) as usize;
                rgba[i..i + 4].copy_from_slice(&c);
            }
        }
    }
    Icon::from_rgba(rgba, N, N).expect("valid icon buffer")
}

impl Tray {
    pub fn new(svc: Service, ctx: egui::Context, quitting: Arc<AtomicBool>) -> Result<Self, String> {
        let menu = Menu::new();
        let open = MenuItem::new("Open HSnap", true, None);
        let items: Vec<(MenuItem, CaptureMode)> = [
            ("Full Screen", CaptureMode::Full),
            ("Window or Control", CaptureMode::Window),
            ("Active Window", CaptureMode::Active),
            ("Region", CaptureMode::Region),
            ("Entire Page with Scrolling", CaptureMode::Scroll),
        ]
        .into_iter()
        .map(|(l, m)| (MenuItem::new(l, true, None), m))
        .collect();
        let exit = MenuItem::new("Exit", true, None);
        let _ = menu.append(&open);
        let _ = menu.append(&PredefinedMenuItem::separator());
        for (item, _) in &items {
            let _ = menu.append(item);
        }
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&exit);

        let (open_id, exit_id) = (open.id().clone(), exit.id().clone());
        let ids: Vec<_> = items.iter().map(|(i, m)| (i.id().clone(), *m)).collect();
        let (svc2, ctx2) = (svc.clone(), ctx.clone());
        MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
            if e.id == open_id {
                show_own(SW_SHOW);
                show_own(SW_RESTORE);
                ctx2.request_repaint();
            } else if e.id == exit_id {
                quitting.store(true, Ordering::Relaxed);
                std::process::exit(0);
            } else if let Some((_, mode)) = ids.iter().find(|(id, _)| *id == e.id) {
                svc2.capture(*mode);
            }
        }));
        TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = e {
                show_own(SW_SHOW);
                show_own(SW_RESTORE);
                ctx.request_repaint();
            }
        }));
        let icon = TrayIconBuilder::new().with_menu(Box::new(menu)).with_tooltip("HSnap").with_icon(icon_image()).build().map_err(|e| e.to_string())?;
        Ok(Self { _icon: icon })
    }
}
