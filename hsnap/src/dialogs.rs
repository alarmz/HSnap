//! Modal-style dialogs: Capture Settings, hot keys, new image, scale, about.

use crate::app::App;
use crate::compose::Compose;
use crate::editor;
use crate::hotkeys::{self, Binding, HotkeyAction};
use crate::service::{Settings, ViewEdit};
use eframe::egui::{self, Color32, Key};

pub enum Dialog {
    None,
    New { w: u32, h: u32 },
    Scale { w: u32, h: u32, keep: bool },
    CaptureSettings { tab: usize, draft: Box<Settings> },
    HotKeys { draft: Box<Settings>, recording: Option<usize> },
    About,
    Startup { draft: Box<Settings>, error: Option<String> },
    PageSetup { draft: Box<crate::print::PageSetup>, prev: Box<crate::print_ui::Preview> },
    PrintPreview { prev: Box<crate::print_ui::Preview>, page: usize },
    Stamps(Box<crate::stamps_ui::Manager>),
    Text,
}

const TABS: [&str; 6] = ["Capture", "Region", "View & Edit", "Copy & Print", "QuickSave", "TextSnap"];

pub fn show(app: &mut App, ctx: &egui::Context) {
    let mut dialog = std::mem::replace(&mut app.dialog, Dialog::None);
    let keep_open = match &mut dialog {
        Dialog::None => false,
        Dialog::About => about(ctx),
        Dialog::New { w, h } => new_image(app, ctx, w, h),
        Dialog::Scale { w, h, keep } => scale(app, ctx, w, h, keep),
        Dialog::CaptureSettings { tab, draft } => capture_settings(app, ctx, tab, draft),
        Dialog::HotKeys { draft, recording } => hot_keys(app, ctx, draft, recording),
        Dialog::Startup { draft, error } => startup(app, ctx, draft, error),
        Dialog::PageSetup { draft, prev } => crate::print_ui::page_setup(app, ctx, draft, prev),
        Dialog::Stamps(m) => crate::stamps_ui::stamps(app, ctx, m),
        Dialog::Text => text_window(app, ctx),
        Dialog::PrintPreview { prev, page } => crate::print_ui::print_preview(app, ctx, prev, page),
    };
    // A handler may have opened a different dialog; do not clobber it.
    if keep_open && matches!(app.dialog, Dialog::None) {
        app.dialog = dialog;
    }
}

fn about(ctx: &egui::Context) -> bool {
    let mut open = true;
    egui::Window::new("About HSnap").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.label("HSnap — a clean-room screen capture tool written in Rust.");
        ui.label("Behaviour follows the HyperSnap 6 user manual; no original code or assets are used.");
    });
    open
}

fn new_image(app: &mut App, ctx: &egui::Context, w: &mut u32, h: &mut u32) -> bool {
    let mut open = true;
    let mut create = false;
    egui::Window::new("New Image").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Width");
            ui.add(egui::DragValue::new(w).range(1..=16000));
            ui.label("Height");
            ui.add(egui::DragValue::new(h).range(1..=16000));
        });
        if ui.button("Create").clicked() {
            create = true;
        }
    });
    if create {
        let bg = image::Rgba([app.bg.r(), app.bg.g(), app.bg.b(), app.bg.a()]);
        app.new_document(image::RgbaImage::from_pixel(*w, *h, bg), "New image".into());
        return false;
    }
    open
}

fn scale(app: &mut App, ctx: &egui::Context, w: &mut u32, h: &mut u32, keep: &mut bool) -> bool {
    let mut open = true;
    let mut apply = false;
    let ratio = app.docs.get(app.current).map(|d| d.image().width() as f32 / d.image().height() as f32).unwrap_or(1.0);
    egui::Window::new("Scale").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.checkbox(keep, "Keep aspect ratio");
        ui.horizontal(|ui| {
            ui.label("Width");
            if ui.add(egui::DragValue::new(w).range(1..=32000)).changed() && *keep {
                *h = ((*w as f32 / ratio).round() as u32).max(1);
            }
            ui.label("Height");
            if ui.add(egui::DragValue::new(h).range(1..=32000)).changed() && *keep {
                *w = ((*h as f32 * ratio).round() as u32).max(1);
            }
        });
        if ui.button("OK").clicked() {
            apply = true;
        }
    });
    if apply {
        let (nw, nh) = (*w, *h);
        app.edit(|d| d.transform(|i| editor::scale(i, nw, nh)));
        return false;
    }
    open
}

fn capture_settings(app: &mut App, ctx: &egui::Context, tab: &mut usize, d: &mut Settings) -> bool {
    let mut open = true;
    let (mut ok, mut cancel, mut apply) = (false, false, false);
    egui::Window::new("Capture Settings").open(&mut open).collapsible(false).default_width(480.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            for (i, t) in TABS.iter().enumerate() {
                ui.selectable_value(tab, i, *t);
            }
        });
        ui.separator();
        match *tab {
            0 => {
                egui::Grid::new("cap").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Delay before capture (ms)");
                    ui.add(egui::DragValue::new(&mut d.delay_ms).range(0..=60_000).speed(50));
                    ui.end_row();
                    ui.label("Auto-scroll refresh time (ms)");
                    ui.add(egui::DragValue::new(&mut d.scroll_refresh_ms).range(50..=5000).speed(10));
                    ui.end_row();
                    ui.label("Default background color");
                    let mut c = Color32::from_rgba_unmultiplied(d.background[0], d.background[1], d.background[2], d.background[3]);
                    if ui.color_edit_button_srgba(&mut c).changed() {
                        d.background = [c.r(), c.g(), c.b(), c.a()];
                    }
                    ui.end_row();
                });
                ui.checkbox(&mut d.bg_transparent, "Transparent (PNG/GIF only)");
                ui.checkbox(&mut d.include_cursor, "Include cursor image");
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "Turn off font smoothing for capture"));
                ui.checkbox(&mut d.play_sound, "Play sound");
                ui.checkbox(&mut d.hide_window, "Hide this window before capture");
                ui.checkbox(&mut d.restore_window, "Restore this window after capture");
                ui.checkbox(&mut d.only_if_changed, "Do automatic actions only if the capture differs from the previous one");
                ui.checkbox(&mut d.mark_modified, "Mark new captures as \"modified\"");
            }
            1 => {
                ui.checkbox(&mut d.show_region_help, "Show help and zoom area during region capture");
                ui.checkbox(&mut d.multi_start_window, "Start multi-region capture with the window/control selector");
                ui.separator();
                ui.label("Multi-region composition");
                ui.radio_value(&mut d.multi_compose, Compose::Preserve, "Preserve each fragment's relative screen position");
                ui.radio_value(&mut d.multi_compose, Compose::TileVertical, "Tile fragments vertically");
                ui.radio_value(&mut d.multi_compose, Compose::TileHorizontal, "Tile fragments horizontally");
                ui.add_enabled_ui(d.multi_compose != Compose::Preserve, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Tiling margin (px)");
                        ui.add(egui::DragValue::new(&mut d.multi_margin).range(0..=500));
                    });
                });
                ui.weak("Default region shape is not implemented yet; gaps use the background color from the Capture tab.");
            }
            2 => {
                ui.radio_value(&mut d.view_edit, ViewEdit::NewWindow, "Create a new window for each capture");
                ui.radio_value(&mut d.view_edit, ViewEdit::Replace, "Replace the image in the current window");
                ui.radio_value(&mut d.view_edit, ViewEdit::DoNotChange, "Do not change current image (clipboard / save only)");
            }
            3 => {
                ui.checkbox(&mut d.auto_copy, "Automatically copy each capture to the clipboard");
                ui.checkbox(&mut d.auto_print, "Automatically print each capture");
                ui.add_enabled(d.auto_print, egui::Checkbox::new(&mut d.auto_print_prompt, "Prompt for printer (otherwise the default printer is used)"));
            }
            5 => {
                ui.label("Text is read through UI Automation where the target exposes it, and by Windows OCR otherwise.");
                ui.radio_value(&mut d.text_mode, crate::textsnap::TextMode::Plain, "Plain text");
                ui.radio_value(&mut d.text_mode, crate::textsnap::TextMode::TabDelimited, "Tab-delimited data (tables; paste into a spreadsheet)");
                ui.add_enabled(false, egui::Checkbox::new(&mut false, "Formatted text (RTF)")).on_disabled_hover_text("Not available: neither OCR nor UI Automation reports fonts");
                ui.separator();
                ui.checkbox(&mut d.text_show_window, "Show captured text in a window");
                ui.add_enabled(d.text_show_window, egui::Checkbox::new(&mut d.text_append, "Append new text to the same window"));
                ui.horizontal(|ui| {
                    ui.label("Additional delay for Text from an Object under Cursor (ms)");
                    ui.add(egui::DragValue::new(&mut d.text_delay_ms).range(0..=30_000).speed(50));
                });
                ui.horizontal(|ui| {
                    ui.label("OCR language");
                    let shown = if d.ocr_lang.is_empty() { "Windows default".to_string() } else { d.ocr_lang.clone() };
                    egui::ComboBox::from_id_salt("ocr-lang").selected_text(shown).show_ui(ui, |ui| {
                        ui.selectable_value(&mut d.ocr_lang, String::new(), "Windows default");
                        for l in ocr_languages() {
                            ui.selectable_value(&mut d.ocr_lang, l.clone(), l.clone());
                        }
                    });
                });
                ui.weak("Auto-scroll for objects under the cursor is not implemented.");
            }
            _ => {
                ui.checkbox(&mut d.quicksave, "Automatically save each capture");
                ui.add_enabled_ui(d.quicksave, |ui| {
                    egui::Grid::new("qs").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                        ui.label("Folder");
                        ui.horizontal(|ui| {
                            ui.text_edit_singleline(&mut d.quicksave_dir);
                            if ui.button("…").clicked() {
                                if let Some(p) = rfd::FileDialog::new().pick_folder() {
                                    d.quicksave_dir = p.to_string_lossy().into_owned();
                                }
                            }
                        });
                        ui.end_row();
                        ui.label("File name");
                        ui.text_edit_singleline(&mut d.quicksave_template);
                        ui.end_row();
                        ui.label("Start at");
                        ui.add(egui::DragValue::new(&mut d.quicksave_start));
                        ui.end_row();
                        ui.label("Stop saving at (blank = no limit)");
                        ui.text_edit_singleline(&mut d.quicksave_stop);
                        ui.end_row();
                        ui.label("Increment by");
                        ui.add(egui::DragValue::new(&mut d.quicksave_step).range(1..=1000));
                        ui.end_row();
                    });
                    ui.checkbox(&mut d.quicksave_loop, "Loop from start to stop number, overwriting old files");
                    ui.horizontal(|ui| {
                        ui.label("Repeat first capture every (s)");
                        ui.add(egui::DragValue::new(&mut d.repeat_secs).range(0.1..=3600.0).speed(0.5));
                    });
                    ui.weak("Use a printf-style name such as Snap%04d.png for fixed padding.");
                });
            }
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
            apply = ui.button("Apply").clicked();
        });
    });
    if ok || apply {
        let mut s = app.settings();
        // Keep hot keys edited in the other dialog.
        let hk = std::mem::take(&mut s.hotkeys);
        s = d.clone();
        s.hotkeys = hk;
        app.apply_settings(s);
    }
    open && !ok && !cancel
}

fn hot_keys(app: &mut App, ctx: &egui::Context, d: &mut Settings, recording: &mut Option<usize>) -> bool {
    let mut open = true;
    let (mut ok, mut cancel, mut reset) = (false, false, false);

    // Record a new binding from the next key press.
    if let Some(idx) = *recording {
        let ev = ctx.input(|i| {
            if i.key_pressed(Key::Escape) {
                return Some(None);
            }
            i.events.iter().find_map(|e| match e {
                egui::Event::Key { key, pressed: true, modifiers, .. } => {
                    let vk = hotkeys::vk_from_egui(*key)?;
                    let is_f = (0x70..=0x7B).contains(&vk);
                    (modifiers.any() || is_f).then(|| Some(Binding { ctrl: modifiers.ctrl, shift: modifiers.shift, alt: modifiers.alt, vk }))
                }
                _ => None,
            })
        });
        if let Some(b) = ev {
            if let Some(b) = b {
                if let Some(slot) = d.hotkeys.get_mut(idx) {
                    slot.1 = b;
                }
            }
            *recording = None;
        }
    }

    let status = app.hotkey_status();
    egui::Window::new("Screen Capture Hot Keys").open(&mut open).collapsible(false).show(ctx, |ui| {
        ui.checkbox(&mut d.hotkeys_enabled, "Activate hot keys");
        ui.separator();
        egui::Grid::new("hk").num_columns(3).spacing([14.0, 6.0]).show(ui, |ui| {
            for (i, (action, b)) in d.hotkeys.iter().enumerate() {
                ui.label(action.label());
                if *recording == Some(i) {
                    ui.label("Press new keys… (Esc cancels)");
                } else {
                    let failed = status.iter().any(|(a, ok)| a == action && !ok);
                    let txt = egui::RichText::new(b.describe());
                    ui.label(if failed { txt.color(Color32::from_rgb(220, 60, 60)) } else { txt });
                }
                if ui.button("Change").clicked() {
                    *recording = Some(i);
                }
                ui.end_row();
            }
        });
        if status.iter().any(|(_, ok)| !ok) {
            ui.colored_label(Color32::from_rgb(220, 60, 60), "Red: another program already uses that key combination.");
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
            reset = ui.button("Defaults").clicked();
        });
    });
    if reset {
        d.hotkeys = hotkeys::defaults();
    }
    if ok {
        let mut s = app.settings();
        s.hotkeys = d.hotkeys.clone();
        s.hotkeys_enabled = d.hotkeys_enabled;
        app.apply_settings(s);
    }
    // Make sure the table is complete if settings predate a new action.
    for a in HotkeyAction::ALL {
        if !d.hotkeys.iter().any(|(x, _)| *x == a) {
            if let Some(def) = hotkeys::defaults().into_iter().find(|(x, _)| *x == a) {
                d.hotkeys.push(def);
            }
        }
    }
    open && !ok && !cancel
}

fn startup(app: &mut App, ctx: &egui::Context, d: &mut Settings, error: &mut Option<String>) -> bool {
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new("Startup and Tray Icon").open(&mut open).collapsible(false).resizable(false).show(ctx, |ui| {
        ui.checkbox(&mut d.autostart, "Auto-Start with Windows");
        ui.add_enabled(d.autostart, egui::Checkbox::new(&mut d.autostart_all_users, "For all users of this computer (needs administrator rights)"));
        ui.checkbox(&mut d.start_minimized, "Always start minimized");
        ui.checkbox(&mut d.show_tray, "Display system tray icon");
        ui.add_enabled(d.show_tray, egui::Checkbox::new(&mut d.keep_in_tray, "Do not exit when the close button is clicked"));
        ui.horizontal(|ui| {
            ui.label("Number of most recent files on File menu");
            ui.add(egui::DragValue::new(&mut d.recent_count).range(0..=16));
        });
        if let Some(e) = error.as_ref() {
            ui.colored_label(Color32::from_rgb(220, 60, 60), e);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if ok {
        let exe = std::env::current_exe().unwrap_or_default();
        let prev = app.settings();
        let mut result = Ok(());
        // Drop the old entry if its scope changed, then apply the wanted state.
        if prev.autostart && prev.autostart_all_users != d.autostart_all_users {
            result = crate::startup::set_enabled("HSnap", false, prev.autostart_all_users, &exe);
        }
        if result.is_ok() {
            result = crate::startup::set_enabled("HSnap", d.autostart, d.autostart_all_users, &exe);
        }
        if let Err(e) = result {
            *error = Some(e);
            return true; // stay open so the message is seen
        }
        let mut s = app.settings();
        s.autostart = d.autostart;
        s.autostart_all_users = d.autostart_all_users;
        s.start_minimized = d.start_minimized;
        s.show_tray = d.show_tray;
        s.keep_in_tray = d.keep_in_tray;
        s.recent_count = d.recent_count;
        s.recent.truncate(d.recent_count as usize);
        app.apply_settings(s);
    }
    open && !ok && !cancel
}

/// Installed OCR language tags, looked up once.
fn ocr_languages() -> &'static Vec<String> {
    static LANGS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    LANGS.get_or_init(crate::ocr::available_languages)
}

fn text_window(app: &mut App, ctx: &egui::Context) -> bool {
    let mut open = true;
    let mut clear = false;
    egui::Window::new("Captured Text").open(&mut open).default_size([480.0, 320.0]).show(ctx, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Copy").clicked() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(app.text_buffer.clone());
                }
            }
            clear = ui.button("Clear").clicked();
        });
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add(egui::TextEdit::multiline(&mut app.text_buffer).desired_width(f32::INFINITY).desired_rows(14));
        });
    });
    if clear {
        app.text_buffer.clear();
    }
    open
}
