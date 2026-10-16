//! Global hot keys on a dedicated message-loop thread; bindings can be changed at run time.

use serde::{Deserialize, Serialize};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::WindowsAndMessaging::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum HotkeyAction {
    FullScreen,
    Window,
    ActiveWindow,
    Region,
    MultiRegion,
    Cursor,
    RepeatLast,
    StopAuto,
}

impl HotkeyAction {
    pub const ALL: [HotkeyAction; 8] = [
        HotkeyAction::FullScreen,
        HotkeyAction::Window,
        HotkeyAction::ActiveWindow,
        HotkeyAction::Region,
        HotkeyAction::MultiRegion,
        HotkeyAction::Cursor,
        HotkeyAction::RepeatLast,
        HotkeyAction::StopAuto,
    ];
    pub fn label(self) -> &'static str {
        match self {
            HotkeyAction::FullScreen => "Full screen",
            HotkeyAction::Window => "Window or control",
            HotkeyAction::ActiveWindow => "Active window",
            HotkeyAction::Region => "Region",
            HotkeyAction::MultiRegion => "Multi-region",
            HotkeyAction::Cursor => "Mouse cursor only",
            HotkeyAction::RepeatLast => "Repeat last capture",
            HotkeyAction::StopAuto => "Stop auto-capture",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Binding {
    pub ctrl: bool,
    pub shift: bool,
    pub alt: bool,
    pub vk: u32,
}

impl Binding {
    pub fn describe(&self) -> String {
        let mut s = String::new();
        if self.ctrl { s += "Ctrl+"; }
        if self.shift { s += "Shift+"; }
        if self.alt { s += "Alt+"; }
        s += &match self.vk {
            0x30..=0x39 | 0x41..=0x5A => (self.vk as u8 as char).to_string(),
            0x70..=0x7B => format!("F{}", self.vk - 0x6F),
            v => format!("0x{v:02X}"),
        };
        s
    }
    fn modifiers(&self) -> HOT_KEY_MODIFIERS {
        let mut m = MOD_NOREPEAT;
        if self.ctrl { m |= MOD_CONTROL; }
        if self.shift { m |= MOD_SHIFT; }
        if self.alt { m |= MOD_ALT; }
        m
    }
}

pub fn defaults() -> Vec<(HotkeyAction, Binding)> {
    let cs = |c: char| Binding { ctrl: true, shift: true, alt: false, vk: c as u32 };
    vec![
        (HotkeyAction::FullScreen, cs('S')),
        (HotkeyAction::Window, cs('W')),
        (HotkeyAction::ActiveWindow, cs('A')),
        (HotkeyAction::Region, cs('R')),
        (HotkeyAction::MultiRegion, cs('M')),
        (HotkeyAction::Cursor, cs('C')),
        (HotkeyAction::RepeatLast, cs('L')),
        (HotkeyAction::StopAuto, Binding { ctrl: false, shift: true, alt: false, vk: VK_F11.0 as u32 }),
    ]
}

/// Virtual-key code for a key the user pressed while recording a binding.
pub fn vk_from_egui(k: eframe::egui::Key) -> Option<u32> {
    use eframe::egui::Key;
    let name = format!("{k:?}");
    if name.len() == 1 {
        return name.chars().next().map(|c| c as u32); // A-Z
    }
    if let Some(n) = name.strip_prefix("Num") {
        return n.parse::<u32>().ok().map(|d| 0x30 + d);
    }
    if let Some(n) = name.strip_prefix('F').and_then(|n| n.parse::<u32>().ok()) {
        if (1..=12).contains(&n) {
            return Some(0x6F + n);
        }
    }
    let _ = Key::A;
    None
}

pub struct Hotkeys {
    pub events: Receiver<HotkeyAction>,
    /// Per-binding registration result of the last `set`, in the order given.
    pub registered: Arc<Mutex<Vec<(HotkeyAction, bool)>>>,
    ctl: Sender<Vec<(HotkeyAction, Binding)>>,
    thread_id: u32,
}

const WM_REBIND: u32 = WM_APP + 1;

impl Hotkeys {
    /// `on_event` is invoked from the hot-key thread after each event (wakes the UI).
    pub fn start(initial: Vec<(HotkeyAction, Binding)>, on_event: impl Fn() + Send + 'static) -> Self {
        let (tx, events) = channel();
        let (ctl, ctl_rx) = channel::<Vec<(HotkeyAction, Binding)>>();
        let (id_tx, id_rx) = channel();
        let registered: Arc<Mutex<Vec<(HotkeyAction, bool)>>> = Default::default();
        let reg = registered.clone();
        std::thread::spawn(move || unsafe {
            let mut msg = MSG::default();
            let _ = PeekMessageW(&mut msg, None, 0, 0, PM_NOREMOVE); // create the queue
            let _ = id_tx.send(GetCurrentThreadId());
            let mut active: Vec<(i32, HotkeyAction)> = vec![];
            let apply = |binds: Vec<(HotkeyAction, Binding)>, active: &mut Vec<(i32, HotkeyAction)>| {
                for (id, _) in active.drain(..) {
                    let _ = UnregisterHotKey(None, id);
                }
                let mut result = vec![];
                for (i, (action, b)) in binds.into_iter().enumerate() {
                    let id = i as i32 + 1;
                    let ok = RegisterHotKey(None, id, b.modifiers(), b.vk).is_ok();
                    if ok {
                        active.push((id, action));
                    }
                    result.push((action, ok));
                }
                *reg.lock().unwrap() = result;
            };
            apply(initial, &mut active);
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                match msg.message {
                    WM_HOTKEY => {
                        if let Some((_, a)) = active.iter().find(|(id, _)| *id == msg.wParam.0 as i32) {
                            if tx.send(*a).is_err() {
                                break;
                            }
                            on_event();
                        }
                    }
                    WM_REBIND => {
                        while let Ok(b) = ctl_rx.try_recv() {
                            apply(b, &mut active);
                        }
                    }
                    _ => {}
                }
            }
        });
        let thread_id = id_rx.recv().unwrap_or(0);
        Self { events, registered, ctl, thread_id }
    }

    /// Replace all bindings (pass an empty list to disable hot keys).
    pub fn set(&self, binds: Vec<(HotkeyAction, Binding)>) {
        let _ = self.ctl.send(binds);
        unsafe {
            let _ = PostThreadMessageW(self.thread_id, WM_REBIND, Default::default(), Default::default());
        }
    }
}
