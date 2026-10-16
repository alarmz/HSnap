//! Global hot keys on a dedicated message-loop thread.

use std::sync::mpsc::{Receiver, Sender, channel};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HotkeyAction {
    Region,
    FullScreen,
    Window,
    RepeatLast,
    StopAuto,
}

const BINDINGS: &[(i32, HOT_KEY_MODIFIERS, u32, HotkeyAction)] = &[
    (1, HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0), b'R' as u32, HotkeyAction::Region),
    (2, HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0), b'S' as u32, HotkeyAction::FullScreen),
    (3, HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0), b'W' as u32, HotkeyAction::Window),
    (4, HOT_KEY_MODIFIERS(MOD_CONTROL.0 | MOD_SHIFT.0), b'L' as u32, HotkeyAction::RepeatLast),
    (5, MOD_SHIFT, VK_F11.0 as u32, HotkeyAction::StopAuto),
];

/// Registers hot keys and forwards them over a channel. `on_event` is invoked
/// from the hot-key thread after each event (used to wake the UI).
pub fn start(on_event: impl Fn() + Send + 'static) -> Receiver<HotkeyAction> {
    let (tx, rx): (Sender<HotkeyAction>, _) = channel();
    std::thread::spawn(move || unsafe {
        for (id, mods, vk, _) in BINDINGS {
            if RegisterHotKey(None, *id, *mods | MOD_NOREPEAT, *vk).is_err() {
                eprintln!("hot key {id} could not be registered (already in use?)");
            }
        }
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            if msg.message == WM_HOTKEY {
                if let Some((_, _, _, action)) = BINDINGS.iter().find(|b| b.0 == msg.wParam.0 as i32) {
                    if tx.send(*action).is_err() {
                        break;
                    }
                    on_event();
                }
            }
        }
    });
    rx
}
