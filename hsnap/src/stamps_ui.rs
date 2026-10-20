//! Stamps manager and the New/Edit Stamp dialog.

use crate::app::App;
use crate::print::Justify;
use crate::stamps::{Anchor, MacroCtx, Placement, Stamp, Store, render};
use eframe::egui::{self, Color32};
use image::RgbaImage;
use std::path::PathBuf;

#[derive(Clone, Copy, PartialEq, Eq)]
enum SaveMode {
    Save,
    Insert,
    Both,
}

const TABS: [&str; 5] = ["Image", "Text", "Frame", "Placement", "Options"];

struct Editor {
    stamp: Stamp,
    picture: Option<RgbaImage>,
    tab: usize,
    mode: SaveMode,
    tex: Option<(String, egui::TextureHandle)>,
}

pub struct Manager {
    dir: PathBuf,
    list: Vec<Stamp>,
    sel: Option<usize>,
    editor: Option<Editor>,
    preview: Option<(String, egui::TextureHandle)>,
    error: Option<String>,
    /// Editing the stamp object at this index of the open image, not a saved stamp.
    instance: Option<usize>,
}

impl Manager {
    pub fn new(dir: PathBuf) -> Self {
        let mut m = Self { dir, list: vec![], sel: None, editor: None, preview: None, error: None, instance: None };
        m.refresh();
        m
    }

    /// Editor for one stamp object on the image; OK changes only that instance.
    pub fn for_instance(dir: PathBuf, index: usize, stamp: Stamp, picture: Option<RgbaImage>) -> Self {
        let mut m = Self::new(dir);
        m.instance = Some(index);
        m.editor = Some(Editor { stamp, picture, tab: 1, mode: SaveMode::Save, tex: None });
        m
    }

    /// Manager that opens straight into the New Stamp dialog (used by the developer hook).
    pub fn with_new_editor(dir: PathBuf) -> Self {
        let mut m = Self::new(dir);
        m.editor = Some(Editor { stamp: Stamp::default(), picture: None, tab: 1, mode: SaveMode::Save, tex: None });
        m
    }

    fn store(&self) -> Store {
        Store { dir: self.dir.clone() }
    }

    fn refresh(&mut self) {
        self.list = self.store().list();
        self.sel = self.sel.filter(|i| *i < self.list.len());
    }
}

fn c32(c: [u8; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(c[0], c[1], c[2], c[3])
}

fn color_edit(ui: &mut egui::Ui, label: &str, c: &mut [u8; 4]) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut v = c32(*c);
        if ui.color_edit_button_srgba(&mut v).changed() {
            *c = [v.r(), v.g(), v.b(), v.a()];
        }
    });
}

fn sample_ctx() -> MacroCtx {
    MacroCtx::now("sample.png", r"C:\Images\sample.png")
}

fn texture(ctx: &egui::Context, slot: &mut Option<(String, egui::TextureHandle)>, key: String, img: &RgbaImage) -> egui::TextureId {
    if slot.as_ref().is_none_or(|(k, _)| *k != key) {
        let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
        *slot = Some((key, ctx.load_texture("stamp", ci, egui::TextureOptions::LINEAR)));
    }
    slot.as_ref().unwrap().1.id()
}

fn show_image(ui: &mut egui::Ui, id: egui::TextureId, size: (u32, u32), max_w: f32) {
    let scale = (max_w / size.0 as f32).min(1.0).max(0.1);
    ui.image(egui::load::SizedTexture::new(id, egui::vec2(size.0 as f32 * scale, size.1 as f32 * scale)));
}

pub fn stamps(app: &mut App, ctx: &egui::Context, m: &mut Manager) -> bool {
    if m.instance.is_some() {
        editor_window(app, ctx, m);
        return m.editor.is_some();
    }
    let mut open = true;
    let mut insert = None::<usize>;
    egui::Window::new("Stamps").open(&mut open).collapsible(false).default_width(520.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Stamps folder");
            ui.label(m.dir.display().to_string());
            if ui.button("Browse…").clicked() {
                if let Some(p) = rfd::FileDialog::new().set_directory(&m.dir).pick_folder() {
                    m.dir = p.clone();
                    m.refresh();
                    let mut s = app.settings();
                    s.stamps_dir = p.to_string_lossy().into_owned();
                    app.apply_settings_quiet(s);
                }
            }
        });
        ui.separator();
        ui.horizontal_top(|ui| {
            egui::ScrollArea::vertical().id_salt("stamp-list").min_scrolled_height(200.0).max_height(260.0).show(ui, |ui| {
                ui.set_min_width(260.0);
                if m.list.is_empty() {
                    ui.weak("No stamps yet. Click New Stamp.");
                }
                let store = Store { dir: m.dir.clone() };
                let mut changed = None;
                for (i, s) in m.list.iter_mut().enumerate() {
                    ui.horizontal(|ui| {
                        if ui.checkbox(&mut s.auto_add, "").on_hover_text("Insert automatically on every new capture").changed() {
                            changed = Some(i);
                        }
                        if ui.selectable_label(m.sel == Some(i), &s.name).clicked() {
                            m.sel = Some(i);
                        }
                    });
                }
                if let Some(i) = changed {
                    let s = m.list[i].clone();
                    let pic = store.picture(&s);
                    if let Err(e) = store.save(&s, pic.as_ref()) {
                        m.error = Some(e);
                    }
                }
            });
            ui.add_space(8.0);
            ui.vertical(|ui| {
                let has = m.sel.is_some();
                if ui.button("New Stamp…").clicked() {
                    let mut st = Stamp::default();
                    let mut n = m.list.len() + 1;
                    while m.list.iter().any(|s| s.name == format!("Stamp {n}")) {
                        n += 1;
                    }
                    st.name = format!("Stamp {n}");
                    m.editor = Some(Editor { stamp: st, picture: None, tab: 0, mode: SaveMode::Save, tex: None });
                }
                if ui.add_enabled(has, egui::Button::new("Edit…")).clicked() {
                    if let Some(s) = m.sel.and_then(|i| m.list.get(i)).cloned() {
                        let picture = m.store().picture(&s);
                        m.editor = Some(Editor { stamp: s, picture, tab: 0, mode: SaveMode::Save, tex: None });
                    }
                }
                if ui.add_enabled(has, egui::Button::new("Preview…")).clicked() {
                    if let Some(s) = m.sel.and_then(|i| m.list.get(i)).cloned() {
                        let pic = m.store().picture(&s);
                        let img = render(&s, pic.as_ref(), &sample_ctx(), None);
                        let mut slot = None;
                        texture(ctx, &mut slot, format!("{}{:?}", s.name, std::time::Instant::now()), &img);
                        m.preview = slot;
                    }
                }
                if ui.add_enabled(has, egui::Button::new("Delete")).clicked() {
                    if let Some(s) = m.sel.and_then(|i| m.list.get(i)).cloned() {
                        match m.store().delete(&s.name) {
                            Ok(()) => {
                                m.sel = None;
                                m.refresh();
                            }
                            Err(e) => m.error = Some(e),
                        }
                    }
                }
                if ui.add_enabled(has && !app.docs.is_empty(), egui::Button::new("Insert")).clicked() {
                    insert = m.sel;
                }
            });
        });
        if let Some(e) = &m.error {
            ui.colored_label(Color32::from_rgb(220, 60, 60), e);
        }
    });
    if let Some(i) = insert {
        if let Some(s) = m.list.get(i).cloned() {
            let pic = m.store().picture(&s);
            app.insert_stamp(&s, pic.as_ref());
            return false;
        }
    }

    // Stamp preview window.
    let mut close_preview = false;
    if let Some((_, tex)) = &m.preview {
        let (id, size) = (tex.id(), tex.size());
        egui::Window::new("Stamp preview").collapsible(false).show(ctx, |ui| {
            show_image(ui, id, (size[0] as u32, size[1] as u32), 480.0);
            close_preview = ui.button("Close").clicked();
        });
    }
    if close_preview {
        m.preview = None;
    }

    editor_window(app, ctx, m);
    open
}

fn editor_window(app: &mut App, ctx: &egui::Context, m: &mut Manager) {
    let instance = m.instance;
    let Some(ed) = m.editor.as_mut() else { return };
    let (mut ok, mut cancel) = (false, false);
    let mut open = true;
    egui::Window::new("New / Edit Stamp").open(&mut open).collapsible(false).default_width(480.0).show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label("Stamp name");
            ui.text_edit_singleline(&mut ed.stamp.name);
        });
        ui.horizontal(|ui| {
            for (i, t) in TABS.iter().enumerate() {
                ui.selectable_value(&mut ed.tab, i, *t);
            }
        });
        ui.separator();
        let s = &mut ed.stamp;
        match ed.tab {
            0 => {
                ui.horizontal(|ui| {
                    if ui.add_enabled(!app.docs.is_empty(), egui::Button::new("From Selection")).clicked() {
                        ed.picture = app.selection_fragment();
                        s.use_image = ed.picture.is_some();
                    }
                    if ui.button("From File…").clicked() {
                        if let Some(p) = rfd::FileDialog::new().add_filter("Images", &["png", "jpg", "jpeg", "bmp", "gif"]).pick_file() {
                            if let Ok(i) = image::open(p) {
                                ed.picture = Some(i.to_rgba8());
                                s.use_image = true;
                            }
                        }
                    }
                    if ui.add_enabled(ed.picture.is_some(), egui::Button::new("Delete Image")).clicked() {
                        ed.picture = None;
                    }
                });
                ui.checkbox(&mut s.use_image, "Use this image");
                if let Some(p) = &ed.picture {
                    let mut slot = None;
                    let id = texture(ctx, &mut slot, "img".into(), p);
                    show_image(ui, id, p.dimensions(), 240.0);
                }
            }
            1 => {
                ui.checkbox(&mut s.use_text, "Use this text");
                ui.add(egui::TextEdit::multiline(&mut s.text).desired_rows(4).desired_width(f32::INFINITY));
                ui.horizontal(|ui| {
                    for (label, mac) in [("Date", "%date%"), ("Time", "%time%"), ("File", "%file%"), ("Path", "%path%"), ("User", "%uname%")] {
                        if ui.button(label).on_hover_text(mac).clicked() {
                            s.text.push_str(mac);
                        }
                    }
                });
                ui.horizontal(|ui| {
                    ui.label("Text height (px)");
                    ui.add(egui::DragValue::new(&mut s.font_px).range(6.0..=200.0));
                    for (j, l) in [(Justify::Left, "Left"), (Justify::Center, "Center"), (Justify::Right, "Right")] {
                        ui.selectable_value(&mut s.align, j, l);
                    }
                });
                color_edit(ui, "Text color", &mut s.text_color);
            }
            2 => {
                egui::Grid::new("frame").num_columns(2).spacing([12.0, 6.0]).show(ui, |ui| {
                    ui.label("Frame width (0 = none)");
                    ui.add(egui::DragValue::new(&mut s.frame_width).range(0..=50));
                    ui.end_row();
                    ui.label("Stamp size (W x H)");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.width).range(0..=4000));
                        ui.add(egui::DragValue::new(&mut s.height).range(0..=4000));
                        if ui.add_enabled(ed.picture.is_some(), egui::Button::new("Set to image size")).clicked() {
                            if let Some(p) = &ed.picture {
                                (s.width, s.height) = p.dimensions();
                            }
                        }
                    });
                    ui.end_row();
                    ui.label("Image position (L, T)");
                    ui.horizontal(|ui| {
                        ui.add(egui::DragValue::new(&mut s.img_left));
                        ui.add(egui::DragValue::new(&mut s.img_top));
                    });
                    ui.end_row();
                    ui.label("Text margins (L T R B)");
                    ui.horizontal(|ui| {
                        for mg in s.margins.iter_mut() {
                            ui.add(egui::DragValue::new(mg).range(0..=500));
                        }
                    });
                    ui.end_row();
                });
                color_edit(ui, "Frame color", &mut s.frame_color);
                color_edit(ui, "Background color", &mut s.bg);
                ui.checkbox(&mut s.transparent_bg, "Make the background color transparent");
                ui.weak("Size 0 x 0 fits the stamp to its content.");
            }
            3 => {
                let mut kind = match s.placement {
                    Placement::AtSelection => 0,
                    Placement::ExtendTop => 1,
                    Placement::ExtendBottom => 2,
                    Placement::OnPicture { .. } => 3,
                };
                let before = kind;
                ui.radio_value(&mut kind, 0, "Place at current selection (top-left corner if none)");
                ui.radio_value(&mut kind, 1, "Extend picture at top (header)");
                ui.radio_value(&mut kind, 2, "Extend picture at bottom (footer)");
                ui.radio_value(&mut kind, 3, "Place on the picture");
                if kind != before {
                    s.placement = match kind {
                        0 => Placement::AtSelection,
                        1 => Placement::ExtendTop,
                        2 => Placement::ExtendBottom,
                        _ => Placement::OnPicture { h: 10, v: 10, from: Anchor::BottomRight },
                    };
                }
                if let Placement::OnPicture { h, v, from } = &mut s.placement {
                    ui.horizontal(|ui| {
                        ui.label("Offset H / V");
                        ui.add(egui::DragValue::new(h));
                        ui.add(egui::DragValue::new(v));
                        egui::ComboBox::from_id_salt("anchor").selected_text(from.label()).show_ui(ui, |ui| {
                            for a in Anchor::ALL {
                                ui.selectable_value(from, a, a.label());
                            }
                        });
                    });
                    ui.weak("Right/bottom anchors: positive offsets move left/up.");
                }
            }
            _ => {
                let mut mask = s.mask_color.is_some();
                if ui.checkbox(&mut mask, "Mask a color of the stamp (make it transparent)").changed() {
                    s.mask_color = mask.then_some([255, 255, 255, 255]);
                }
                if let Some(mc) = &mut s.mask_color {
                    color_edit(ui, "Mask color", mc);
                }
                ui.horizontal(|ui| {
                    ui.label("Blend: opaque");
                    ui.add(egui::Slider::new(&mut s.blend, 0..=100));
                    ui.label("transparent");
                });
                ui.checkbox(&mut s.auto_add, "Auto-add this stamp to all captured images");
            }
        }
        ui.separator();
        // Live preview of the composed stamp.
        let key = format!("{}{}", serde_json::to_string(&*s).unwrap_or_default(), ed.picture.as_ref().map_or(0, |p| p.width() * 31 + p.height()));
        let img = render(s, ed.picture.as_ref(), &sample_ctx(), None);
        let id = texture(ctx, &mut ed.tex, key, &img);
        show_image(ui, id, img.dimensions(), 440.0);
        ui.separator();
        if instance.is_some() {
            ui.weak("Changes apply to this stamp on the image only; the saved stamp is not modified.");
        } else {
            ui.horizontal(|ui| {
                ui.radio_value(&mut ed.mode, SaveMode::Save, "Save stamp");
                ui.radio_value(&mut ed.mode, SaveMode::Insert, "Insert on picture");
                ui.radio_value(&mut ed.mode, SaveMode::Both, "Insert and save");
            });
        }
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });

    if ok && instance.is_some() {
        let (stamp, pic) = (ed.stamp.clone(), ed.picture.clone());
        app.update_stamp_instance(instance.unwrap_or(0), &stamp, pic.as_ref());
        m.editor = None;
    } else if ok {
        let (stamp, pic, mode) = (ed.stamp.clone(), ed.picture.clone(), ed.mode);
        let mut done = true;
        if mode != SaveMode::Insert {
            if let Err(e) = m.store().save(&stamp, pic.as_ref()) {
                m.error = Some(e);
                done = false;
            }
        }
        if done && mode != SaveMode::Save {
            app.insert_stamp(&stamp, pic.as_ref());
        }
        if done {
            m.editor = None;
            m.refresh();
        }
    } else if cancel || !open {
        m.editor = None;
    }
}
