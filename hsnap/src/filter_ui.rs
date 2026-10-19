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
    subst_sel: Option<usize>,
}

impl FilterDialog {
    pub fn new(kind: Kind, src: RgbaImage, sel: Option<Sel>, bg: [u8; 4], saved: &crate::service::Settings) -> Self {
        // Preview on a 1:1 crop so pixel-sized parameters look the same as in the result.
        let (w, h) = (src.width().min(360), src.height().min(260));
        let preview_src = image::imageops::crop_imm(&src, (src.width() - w) / 2, (src.height() - h) / 2, w, h).to_image();
        Self {
            kind,
            sel,
            bg,
            preview_src,
            tex: None,
            subst_sel: None,
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
    ui.label("Color replacement table  (> replace,  <> swap)");
    egui::ScrollArea::vertical().id_salt("subst").max_height(120.0).show(ui, |ui| {
        for (i, e) in t.entries.iter().enumerate() {
            ui.horizontal(|ui| {
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, c32(e.old));
                let label = format!("{}  ({},{},{}) {} ({},{},{})", i + 1, e.old[0], e.old[1], e.old[2], if e.swap { "<>" } else { ">" }, e.new[0], e.new[1], e.new[2]);
                if ui.selectable_label(d.subst_sel == Some(i), label).clicked() {
                    d.subst_sel = Some(i);
                }
                let (r, _) = ui.allocate_exact_size(egui::vec2(16.0, 16.0), egui::Sense::hover());
                ui.painter().rect_filled(r, 2.0, c32(e.new));
            });
        }
    });
    ui.horizontal(|ui| {
        if ui.button("Add").clicked() {
            t.entries.push(SubstEntry::default());
            d.subst_sel = Some(t.entries.len() - 1);
        }
        if ui.add_enabled(d.subst_sel.is_some(), egui::Button::new("Delete")).clicked() {
            if let Some(i) = d.subst_sel.take().filter(|i| *i < t.entries.len()) {
                t.entries.remove(i);
            }
        }
        if ui.add_enabled(!t.entries.is_empty(), egui::Button::new("Delete All")).clicked() {
            t.entries.clear();
            d.subst_sel = None;
        }
        if ui.button("Open List…").clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Color list", &["json"]).pick_file() {
                if let Some(loaded) = std::fs::read_to_string(p).ok().and_then(|s| serde_json::from_str::<Vec<SubstEntry>>(&s).ok()) {
                    t.entries = loaded;
                    d.subst_sel = None;
                }
            }
        }
        if ui.add_enabled(!t.entries.is_empty(), egui::Button::new("Save List…")).clicked() {
            if let Some(p) = rfd::FileDialog::new().add_filter("Color list", &["json"]).set_file_name("colors.json").save_file() {
                let _ = std::fs::write(p, serde_json::to_string_pretty(&t.entries).unwrap_or_default());
            }
        }
    });
    if let Some(e) = d.subst_sel.and_then(|i| t.entries.get_mut(i)) {
        ui.separator();
        rgb_edit(ui, "Old color", &mut e.old);
        rgb_edit(ui, "New color", &mut e.new);
        ui.horizontal(|ui| {
            ui.radio_value(&mut e.swap, false, "Replace  >");
            ui.radio_value(&mut e.swap, true, "Swap  <>");
        });
    }
    ui.add(egui::Slider::new(&mut t.tolerance, 0.0..=50.0).text("Color matching tolerance (%)"));
    ui.checkbox(&mut t.auto_apply, "Automatically apply to all captured images");
}

pub fn show(app: &mut App, ctx: &egui::Context, d: &mut FilterDialog) -> bool {
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new(d.kind.title()).open(&mut open).collapsible(false).default_width(460.0).show(ctx, |ui| {
        controls(ui, d);
        ui.separator();
        // Live preview of the effect on a 1:1 crop of the image.
        let key = format!("{:?}{}{}{}{}{}{}{}{}{}{}", d.kind, d.p.shear.0, d.p.shear.1, d.p.mosaic.0, d.p.emboss.0, d.p.sharp, d.p.halftone, serde_json::to_string(&d.p.shadow).unwrap_or_default(), serde_json::to_string(&d.p.frame).unwrap_or_default(), serde_json::to_string(&d.p.colorres).unwrap_or_default(), serde_json::to_string(&(&d.p.correct, &d.p.subst)).unwrap_or_default())
            + &format!("{}{}{}", d.p.mosaic.1, d.p.emboss.1, d.p.apply_now);
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
            cancel = ui.button("Cancel").clicked();
        });
    });
    if ok {
        let sel = match d.kind {
            Kind::Shadow | Kind::Frame if !d.p.apply_now => None,
            _ => d.sel,
        };
        let (kind, p, bg) = (d.kind, d.p.clone(), d.bg);
        app.edit(|doc| doc.transform(|img| run(kind, &p, &img, sel, bg)));
        // Remember the parameters that persist (auto-add options included).
        let mut s = app.settings();
        s.shadow = p.shadow;
        s.frame = p.frame;
        s.subst = p.subst;
        app.apply_settings_quiet(s);
    }
    open && !ok && !cancel
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
