//! Parameter dialog (with live preview) for the Image and Color menu filters.

use crate::app::App;
use crate::filters::*;
use eframe::egui::{self, Color32};
use image::RgbaImage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Shear,
    Mosaic,
    Emboss,
    SharpenBlur,
    Shadow,
    Frame,
    ColorRes,
    Halftone,
    Correct,
    Substitute,
}

impl Kind {
    fn title(self) -> &'static str {
        match self {
            Kind::Shear => "Shear",
            Kind::Mosaic => "Mosaic",
            Kind::Emboss => "Emboss",
            Kind::SharpenBlur => "Sharpen or Blur",
            Kind::Shadow => "Shadow",
            Kind::Frame => "Frame",
            Kind::ColorRes => "Color Resolution",
            Kind::Halftone => "Halftone",
            Kind::Correct => "Color Correction",
            Kind::Substitute => "Substitute Colors",
        }
    }
}

#[derive(Clone)]
struct Params {
    shear: (f32, f32),
    mosaic: (u32, bool),
    emboss: (f32, f32),
    sharp: f32,
    shadow: ShadowParams,
    frame: FrameParams,
    colorres: ColorResParams,
    halftone: f32,
    correct: CorrectParams,
    subst: SubstTable,
    /// Shadow and Frame: apply around the selection (or the whole image).
    apply_now: bool,
}

pub struct FilterDialog {
    kind: Kind,
    sel: Option<Sel>,
    bg: [u8; 4],
    preview_src: RgbaImage,
    tex: Option<(String, egui::TextureHandle)>,
    p: Params,
    /// Rows selected in the substitution table (Ctrl or Shift for several).
    subst_sel: Vec<usize>,
    /// How many times Apply has changed the open image, so Revert and Cancel can undo them.
    applied: u32,
    last_applied: Option<String>,
}

impl FilterDialog {
    pub fn new(kind: Kind, src: RgbaImage, sel: Option<Sel>, bg: [u8; 4], saved: &crate::service::Settings) -> Self {
        // Preview on a 1:1 crop so pixel-sized parameters look the same as in the result.
        let preview_src = preview_crop(&src);
        Self {
            kind,
            sel,
            bg,
            preview_src,
            tex: None,
            subst_sel: vec![],
            applied: 0,
            last_applied: None,
            p: Params {
                shear: (0.0, 0.0),
                mosaic: (8, true),
                emboss: (45.0, 2.0),
                sharp: 40.0,
                shadow: saved.shadow.clone(),
                frame: saved.frame.clone(),
                colorres: ColorResParams::default(),
                halftone: 6.0,
                correct: CorrectParams::default(),
                subst: saved.subst.clone(),
                apply_now: true,
            },
        }
    }
}

/// Run the chosen filter. Size-preserving filters honour `sel`; Shear ignores it.
fn run(kind: Kind, p: &Params, img: &RgbaImage, sel: Option<Sel>, bg: [u8; 4]) -> RgbaImage {
    match kind {
        Kind::Shear => shear(img, p.shear.0, p.shear.1, bg),
        Kind::Mosaic => on_region(img, sel, |i| mosaic(&i, p.mosaic.0, p.mosaic.1)),
        Kind::Emboss => on_region(img, sel, |i| emboss(&i, p.emboss.0, p.emboss.1)),
        Kind::SharpenBlur => on_region(img, sel, |i| sharpen_blur(&i, p.sharp)),
        Kind::Shadow => shadow(img, sel, &p.shadow),
        Kind::Frame => frame(img, sel, &p.frame),
        Kind::ColorRes => on_region(img, sel, |i| color_resolution(&i, &p.colorres)),
        Kind::Halftone => on_region(img, sel, |i| halftone(&i, p.halftone)),
        Kind::Correct => on_region(img, sel, |i| color_correction(&i, &p.correct)),
        Kind::Substitute => on_region(img, sel, |i| substitute(&i, &p.subst)),
    }
}

fn c32(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn rgb_edit(ui: &mut egui::Ui, label: &str, c: &mut [u8; 3]) {
    ui.horizontal(|ui| {
        ui.label(label);
        let mut v = c32(*c);
        if ui.color_edit_button_srgba(&mut v).changed() {
            *c = [v.r(), v.g(), v.b()];
        }
    });
}

fn rgba_edit(ui: &mut egui::Ui, label: &str, c: &mut [u8; 4]) {
    let mut rgb = [c[0], c[1], c[2]];
    rgb_edit(ui, label, &mut rgb);
    *c = [rgb[0], rgb[1], rgb[2], 255];
}

fn controls(ui: &mut egui::Ui, d: &mut FilterDialog) {
    let has_sel = d.sel.is_some();
    match d.kind {
        Kind::Shear => {
            ui.add(egui::Slider::new(&mut d.p.shear.0, -60.0..=60.0).text("Horizontal (degrees)"));
            ui.add(egui::Slider::new(&mut d.p.shear.1, -60.0..=60.0).text("Vertical (degrees)"));
            ui.weak("Revealed corners use the background color.");
        }
        Kind::Mosaic => {
            ui.add(egui::Slider::new(&mut d.p.mosaic.0, 2..=64).text("Tile size (px)"));
            ui.checkbox(&mut d.p.mosaic.1, "Draw tile borders");
        }
        Kind::Emboss => {
            ui.add(egui::Slider::new(&mut d.p.emboss.0, 0.0..=359.0).text("Light direction (degrees)"));
            ui.add(egui::Slider::new(&mut d.p.emboss.1, 0.5..=8.0).text("Depth"));
        }
        Kind::SharpenBlur => {
            ui.add(egui::Slider::new(&mut d.p.sharp, -100.0..=100.0).text("Blur  <  >  Sharpen"));
        }
        Kind::Shadow => {
            let s = &mut d.p.shadow;
            ui.add(egui::Slider::new(&mut s.size, 0..=40).text("Shadow size"));
            ui.add(egui::Slider::new(&mut s.shade, 0..=100).text("Inner shade"));
            ui.add(egui::Slider::new(&mut s.diffusion, 0..=100).text("Diffusion"));
            ui.add(egui::Slider::new(&mut s.direction, 0.0..=359.0).text("Direction (0 = right, 90 = down)"));
            rgba_edit(ui, "Shadow background color", &mut s.bg);
            ui.add_enabled(has_sel, egui::Checkbox::new(&mut d.p.apply_now, "Add around the current image selection"));
            ui.checkbox(&mut s.auto_shapes, "Auto-add this shadow to newly drawn shapes");
            ui.checkbox(&mut s.auto_add, "Auto-add this shadow to all captured images");
        }
        Kind::Frame => {
            let f = &mut d.p.frame;
            ui.add(egui::Slider::new(&mut f.total, 1..=60).text("Total frame width"));
            rgba_edit(ui, "Frame color", &mut f.color);
            ui.checkbox(&mut f.three_d, "3D effect");
            ui.add_enabled_ui(f.three_d, |ui| {
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.outer).range(0..=30).prefix("Outer band "));
                    ui.radio_value(&mut f.outer_raised, true, "Raised");
                    ui.radio_value(&mut f.outer_raised, false, "Sunken");
                });
                ui.horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut f.inner).range(0..=30).prefix("Inner band "));
                    ui.radio_value(&mut f.inner_raised, true, "Raised");
                    ui.radio_value(&mut f.inner_raised, false, "Sunken");
                });
            });
            ui.add_enabled(has_sel, egui::Checkbox::new(&mut d.p.apply_now, "Add around the current image selection"));
            ui.checkbox(&mut f.auto_add, "Auto-add this frame to all captured images");
        }
        Kind::ColorRes => {
            let c = &mut d.p.colorres;
            egui::ComboBox::from_label("Bits per pixel").selected_text(c.bpp.to_string()).show_ui(ui, |ui| {
                for b in [1u8, 2, 3, 4, 5, 6, 7, 8, 16, 24, 32] {
                    ui.selectable_value(&mut c.bpp, b, b.to_string());
                }
            });
            ui.label("Dithering");
            ui.horizontal(|ui| {
                ui.radio_value(&mut c.dither, Dither::None, "None");
                ui.radio_value(&mut c.dither, Dither::FloydSteinberg, "Floyd-Steinberg");
                ui.radio_value(&mut c.dither, Dither::Ordered, "Ordered");
            });
            ui.label("Palette (8 bits per pixel or less)");
            ui.horizontal(|ui| {
                ui.radio_value(&mut c.palette, Palette::Optimized, "Optimized");
                ui.radio_value(&mut c.palette, Palette::StdWindows, "Std. Windows");
                ui.radio_value(&mut c.palette, Palette::Netscape, "Netscape");
                ui.radio_value(&mut c.palette, Palette::BlackWhite, "Black & White");
            });
        }
        Kind::Halftone => {
            ui.add(egui::Slider::new(&mut d.p.halftone, 3.0..=24.0).text("Screen size (px)"));
        }
        Kind::Correct => {
            let c = &mut d.p.correct;
            ui.add(egui::Slider::new(&mut c.brightness, -100.0..=100.0).text("Brightness"));
            ui.add(egui::Slider::new(&mut c.contrast, -100.0..=100.0).text("Contrast"));
            ui.add(egui::Slider::new(&mut c.hue, -180.0..=180.0).text("Hue (degrees)"));
            ui.add(egui::Slider::new(&mut c.saturation, -100.0..=100.0).text("Saturation"));
            ui.add(egui::Slider::new(&mut c.gamma, 0.2..=4.0).text("Gamma"));
            if ui.button("Reset").clicked() {
                *c = CorrectParams::default();
            }
        }
        Kind::Substitute => substitute_controls(ui, d),
    }
    if matches!(d.kind, Kind::Mosaic | Kind::Emboss | Kind::SharpenBlur | Kind::ColorRes | Kind::Halftone | Kind::Correct | Kind::Substitute) {
        ui.weak(if has_sel { "Applies to the current selection." } else { "Applies to the whole image." });
    }
}

fn substitute_controls(ui: &mut egui::Ui, d: &mut FilterDialog) {
    let t = &mut d.p.subst;
    let sel = &mut d.subst_sel;
    ui.label("Color replacement table  (> replace,  <> swap).  Ctrl-click or Shift-click selects several rows.");
    egui::ScrollArea::vertical().id_salt("subst").max_height(120.0).show(ui, |ui| {
        for i in 0..t.entries.len() {
            let e = t.entries[i].clone();
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, c32(e.old));
                let label = format!("{}  ({},{},{}) {} ({},{},{})", i + 1, e.old[0], e.old[1], e.old[2], if e.swap { "<>" } else { ">" }, e.new[0], e.new[1], e.new[2]);
                if ui.selectable_label(sel.contains(&i), label).clicked() {
                    let m = ui.input(|inp| inp.modifiers);
                    select_click(sel, i, m.command, m.shift);
                }
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, c32(e.new));
            });
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Add").clicked() {
            t.entries.push(SubstEntry::default());
            *sel = vec![t.entries.len() - 1];
        }
        if ui.add_enabled(!sel.is_empty(), egui::Button::new("Delete")).clicked() {
            let mut gone = std::mem::take(sel);
            gone.sort_unstable_by(|a, b| b.cmp(a));
            for i in gone {
                if i < t.entries.len() {
                    t.entries.remove(i);
                }
            }
        }
        if ui.add_enabled(!t.entries.is_empty(), egui::Button::new("Delete All")).clicked() {
            t.entries.clear();
            sel.clear();
        }
        if ui.button("Open List…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Color list", &["json"]).pick_file() {
                if let Some(loaded) = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Vec<SubstEntry>>(&s).ok()) {
                    t.entries = loaded;
                    sel.clear();
                }
            }
        }
        if ui.add_enabled(!t.entries.is_empty(), egui::Button::new("Save List…")).clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Color list", &["json"]).set_file_name("colors.json").save_file() {
                let _ = std::fs::write(p, serde_json::to_string_pretty(&t.entries).unwrap_or_default());
            }
        }
    });
    sel.retain(|i| *i < t.entries.len());
    if let Some(&first) = sel.first() {
        ui.separator();
        if sel.len() > 1 {
            ui.label(format!("{} rows selected: changes below apply to all of them.", sel.len()));
        }
        let before = t.entries[first].clone();
        let mut edit = before.clone();
        rgb_edit(ui, "Old color", &mut edit.old);
        rgb_edit(ui, "New color", &mut edit.new);
        ui.horizontal(|ui| {
            ui.radio_value(&mut edit.swap, false, "Replace  >");
            ui.radio_value(&mut edit.swap, true, "Swap  <>");
        });
        propagate_edit(&mut t.entries, sel, &before, &edit);
    }
    ui.add(egui::Slider::new(&mut t.tolerance, 0.0..=50.0).text("Color matching tolerance (%)"));
    ui.checkbox(&mut t.auto_apply, "Automatically apply to all captured images");
}

/// Row selection in the substitution table: plain click selects one row, Ctrl toggles a row,
/// Shift selects the range from the last selected row.
fn select_click(sel: &mut Vec<usize>, i: usize, ctrl: bool, shift: bool) {
    if ctrl {
        match sel.iter().position(|s| *s == i) {
            Some(p) => {
                sel.remove(p);
            }
            None => sel.push(i),
        }
    } else if shift {
        let from = sel.last().copied().unwrap_or(i);
        *sel = (from.min(i)..=from.max(i)).collect();
    } else {
        *sel = vec![i];
    }
}

/// The user edited the first selected row from `before` to `edit`; copy only the fields they changed
/// to every selected row.
fn propagate_edit(entries: &mut [SubstEntry], sel: &[usize], before: &SubstEntry, edit: &SubstEntry) {
    for &i in sel {
        let Some(row) = entries.get_mut(i) else { continue };
        if edit.old != before.old {
            row.old = edit.old;
        }
        if edit.new != before.new {
            row.new = edit.new;
        }
        if edit.swap != before.swap {
            row.swap = edit.swap;
        }
    }
}

/// A centred 1:1 crop used for the live preview.
fn preview_crop(src: &RgbaImage) -> RgbaImage {
    let (w, h) = (src.width().min(360), src.height().min(260));
    image::imageops::crop_imm(src, (src.width() - w) / 2, (src.height() - h) / 2, w, h).to_image()
}

/// Everything that changes the result, as a string: used to cache the preview and to know whether
/// the current settings were already applied.
fn params_key(d: &FilterDialog) -> String {
    format!(
        "{:?}{}{}{}{}{}{}{}{}{}{}{}{}{}",
        d.kind,
        d.p.shear.0,
        d.p.shear.1,
        d.p.mosaic.0,
        d.p.emboss.0,
        d.p.sharp,
        d.p.halftone,
        serde_json::to_string(&d.p.shadow).unwrap_or_default(),
        serde_json::to_string(&d.p.frame).unwrap_or_default(),
        serde_json::to_string(&d.p.colorres).unwrap_or_default(),
        serde_json::to_string(&(&d.p.correct, &d.p.subst)).unwrap_or_default(),
        d.p.mosaic.1,
        d.p.emboss.1,
        d.p.apply_now
    )
}

/// Apply the dialog's settings to the open image (one undo step) and remember the parameters that persist.
fn apply_now(app: &mut App, d: &mut FilterDialog) {
    let sel = match d.kind {
        Kind::Shadow | Kind::Frame if !d.p.apply_now => None,
        _ => d.sel,
    };
    let (kind, p, bg) = (d.kind, d.p.clone(), d.bg);
    let before = app.docs.get(app.current).map(|doc| doc.image().dimensions());
    app.edit(|doc| doc.transform(|img| run(kind, &p, &img, sel, bg)));
    d.applied += 1;
    d.last_applied = Some(params_key(d));
    let mut s = app.settings();
    s.shadow = p.shadow;
    s.frame = p.frame;
    s.subst = p.subst;
    app.apply_settings_quiet(s);
    // The preview now starts from the changed image.
    if let Some(doc) = app.docs.get(app.current) {
        let img = doc.flatten();
        if Some(img.dimensions()) != before {
            d.sel = None; // the canvas changed size, so the old selection no longer means anything
        }
        d.preview_src = preview_crop(&img);
        d.tex = None;
    }
}

/// Undo everything Apply did while the dialog was open.
fn revert_applied(app: &mut App, d: &mut FilterDialog) {
    for _ in 0..d.applied {
        app.edit(|doc| doc.undo());
    }
    d.applied = 0;
    d.last_applied = None;
    if let Some(doc) = app.docs.get(app.current) {
        d.preview_src = preview_crop(&doc.flatten());
        d.tex = None;
    }
}

pub fn show(app: &mut App, ctx: &egui::Context, d: &mut FilterDialog) -> bool {
    let mut open = true;
    let (mut ok, mut cancel, mut apply, mut revert) = (false, false, false, false);
    egui::Window::new(d.kind.title()).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        controls(ui, d);
        ui.separator();
        // Live preview of the effect on a 1:1 crop of the image.
        let key = params_key(d);
        if d.tex.as_ref().is_none_or(|(k, _)| *k != key) {
            let out = run(d.kind, &d.p, &d.preview_src, None, d.bg);
            let ci = egui::ColorImage::from_rgba_unmultiplied([out.width() as usize, out.height() as usize], out.as_raw());
            d.tex = Some((key, ctx.load_texture("filter-preview", ci, egui::TextureOptions::NEAREST)));
        }
        if let Some((_, tex)) = &d.tex {
            let size = tex.size_vec2();
            let scale = (420.0 / size.x).min(1.0);
            ui.image(egui::load::SizedTexture::new(tex.id(), size * scale));
        }
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            apply = ui.button("Apply").on_hover_text("Apply to the image and keep this window open").clicked();
            revert = ui.add_enabled(d.applied > 0, egui::Button::new("Revert")).on_hover_text("Undo what Apply did").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    // Settings that were already applied are not applied a second time.
    if (ok || apply) && d.last_applied.as_deref() != Some(params_key(d).as_str()) {
        apply_now(app, d);
    }
    if revert {
        revert_applied(app, d);
    }
    if ok {
        return false;
    }
    if cancel || !open {
        revert_applied(app, d); // Cancel really cancels, including earlier Applies
        return false;
    }
    true
}

/// Apply the filters the user set to run on every capture (auto-add shadow, frame, substitution).
pub fn auto_filters(img: RgbaImage, s: &crate::service::Settings) -> RgbaImage {
    let mut img = img;
    if s.subst.auto_apply && !s.subst.entries.is_empty() {
        img = substitute(&img, &s.subst);
    }
    if s.shadow.auto_add {
        img = shadow(&img, None, &s.shadow);
    }
    if s.frame.auto_add {
        img = frame(&img, None, &s.frame);
    }
    img
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(old: u8, new: u8, swap: bool) -> SubstEntry {
        SubstEntry { old: [old; 3], new: [new; 3], swap }
    }

    #[test]
    fn click_ctrl_and_shift_select_rows() {
        let mut sel = vec![];
        select_click(&mut sel, 2, false, false);
        assert_eq!(sel, [2]);
        select_click(&mut sel, 5, true, false); // Ctrl adds
        assert_eq!(sel, [2, 5]);
        select_click(&mut sel, 2, true, false); // Ctrl again removes
        assert_eq!(sel, [5]);
        select_click(&mut sel, 8, false, true); // Shift: range from the last selected
        assert_eq!(sel, [5, 6, 7, 8]);
        select_click(&mut sel, 1, false, false); // plain click resets
        assert_eq!(sel, [1]);
    }

    #[test]
    fn editing_one_row_changes_only_the_edited_fields_of_all_selected_rows() {
        let mut rows = vec![entry(1, 10, false), entry(2, 20, false), entry(3, 30, false)];
        let before = rows[0].clone();
        let mut edit = before.clone();
        edit.new = [99; 3]; // set the same new colour for the selected rows
        propagate_edit(&mut rows, &[0, 2], &before, &edit);
        assert_eq!(rows[0].new, [99; 3]);
        assert_eq!(rows[2].new, [99; 3]);
        assert_eq!(rows[2].old, [3; 3]); // untouched fields keep their own values
        assert_eq!(rows[1].new, [20; 3]); // unselected row unchanged
        // Switch two rows from Replace to Swap at once.
        let before = rows[0].clone();
        let mut edit = before.clone();
        edit.swap = true;
        propagate_edit(&mut rows, &[0, 1], &before, &edit);
        assert!(rows[0].swap && rows[1].swap && !rows[2].swap);
        // Stale indices are ignored.
        propagate_edit(&mut rows, &[7], &before, &edit);
    }
}
