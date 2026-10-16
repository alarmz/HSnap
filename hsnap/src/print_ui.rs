//! Page Setup and Print Preview dialogs (egui) with a shared page preview.

use crate::app::App;
use crate::print::{Fit, Justify, PageSetup, Paper, plan_preview, prepare_image};
use eframe::egui::{self, Color32, Pos2, Sense, Stroke, vec2};
use image::RgbaImage;
use std::sync::Arc;

/// Source bitmap plus a texture made with the colour options of the setup applied.
pub struct Preview {
    src: Arc<RgbaImage>,
    tex: Option<egui::TextureHandle>,
    key: (bool, bool),
    pub name: String,
}

impl Preview {
    pub fn new(src: RgbaImage, name: String) -> Self {
        Self { src: Arc::new(src), tex: None, key: (false, false), name }
    }

    /// Stand-in used by Page Setup when no image is open.
    pub fn placeholder() -> Self {
        let img = RgbaImage::from_fn(640, 480, |x, y| image::Rgba([(x / 3) as u8, (y / 2) as u8, 200, 255]));
        Self::new(img, "sample.png".into())
    }

    fn texture(&mut self, ctx: &egui::Context, setup: &PageSetup) -> egui::TextureId {
        let key = (setup.invert, setup.black_and_white);
        if self.tex.is_none() || self.key != key {
            let img = prepare_image(&self.src, setup);
            let ci = egui::ColorImage::from_rgba_unmultiplied([img.width() as usize, img.height() as usize], img.as_raw());
            self.tex = Some(ctx.load_texture("print-preview", ci, egui::TextureOptions::LINEAR));
            self.key = key;
        }
        self.tex.as_ref().unwrap().id()
    }
}

/// Draw one page; returns the number of pages.
fn draw_page(ui: &mut egui::Ui, prev: &mut Preview, setup: &PageSetup, page_idx: usize, max: egui::Vec2) -> usize {
    let (iw, ih) = prev.src.dimensions();
    let (page, pages) = plan_preview(setup, iw, ih);
    let scale = (max.x / page.0).min(max.y / page.1).max(0.05);
    let (rect, _) = ui.allocate_exact_size(vec2(page.0 * scale, page.1 * scale), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect.translate(vec2(3.0, 3.0)), 0.0, Color32::from_black_alpha(60));
    p.rect_filled(rect, 0.0, Color32::WHITE);
    let tex = prev.texture(ui.ctx(), setup);
    let idx = page_idx.min(pages.len() - 1);
    if let Some(pg) = pages.get(idx) {
        let r = egui::Rect::from_min_size(rect.min + vec2(pg.x, pg.y) * scale, vec2(pg.w, pg.h) * scale);
        let uv = egui::Rect::from_min_max(Pos2::new(0.0, pg.src_y as f32 / ih as f32), Pos2::new(1.0, (pg.src_y + pg.src_h) as f32 / ih as f32));
        p.image(tex, r, uv, Color32::WHITE);
        if setup.draw_frame {
            p.rect_stroke(r.expand(1.0), 0.0, Stroke::new(1.0, Color32::BLACK), egui::StrokeKind::Outside);
        }
    }
    let font = egui::FontId::proportional((setup.font_pt * scale).max(6.0));
    let usable = (page.0 - setup.margins[0] * 72.0 - setup.margins[2] * 72.0) * scale;
    for (text, just, y) in [(&setup.header, setup.header_justify, setup.margins[1] * 72.0), (&setup.footer, setup.footer_justify, page.1 - setup.margins[3] * 72.0 - setup.font_pt * 1.2)] {
        if text.is_empty() {
            continue;
        }
        let t = crate::print::expand_text(text, idx + 1, pages.len(), &prev.name);
        let x0 = rect.min.x + setup.margins[0] * 72.0 * scale;
        let (x, anchor) = match just {
            Justify::Left => (x0, egui::Align2::LEFT_TOP),
            Justify::Center => (x0 + usable / 2.0, egui::Align2::CENTER_TOP),
            Justify::Right => (x0 + usable, egui::Align2::RIGHT_TOP),
        };
        p.text(Pos2::new(x, rect.min.y + y * scale), anchor, t, font.clone(), Color32::BLACK);
    }
    pages.len()
}

pub fn page_setup(app: &mut App, ctx: &egui::Context, d: &mut PageSetup, prev: &mut Preview) -> bool {
    let mut open = true;
    let (mut ok, mut cancel) = (false, false);
    egui::Window::new("Page Setup").open(&mut open).collapsible(false).show(ctx, |ui| {
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(300.0);
                ui.horizontal(|ui| {
                    ui.label("Paper");
                    egui::ComboBox::from_id_salt("paper").selected_text(d.paper.name()).show_ui(ui, |ui| {
                        for p in Paper::ALL {
                            ui.selectable_value(&mut d.paper, p, p.name());
                        }
                    });
                });
                ui.checkbox(&mut d.best_fit, "Best fit (choose orientation from the image)");
                ui.add_enabled_ui(!d.best_fit, |ui| {
                    ui.horizontal(|ui| {
                        ui.radio_value(&mut d.landscape, false, "Portrait");
                        ui.radio_value(&mut d.landscape, true, "Landscape");
                    });
                });
                ui.checkbox(&mut d.center, "Center image");
                ui.horizontal(|ui| {
                    ui.label("Margins (in) L T R B");
                    for m in d.margins.iter_mut() {
                        ui.add(egui::DragValue::new(m).range(0.0..=3.0).speed(0.05).fixed_decimals(2));
                    }
                });
                ui.separator();
                ui.radio_value(&mut d.fit, Fit::FillPage, "Auto-fit to fill the page");
                ui.radio_value(&mut d.fit, Fit::PageWidth, "Auto-fit to page width (may use several pages)");
                ui.horizontal(|ui| {
                    ui.radio_value(&mut d.fit, Fit::Scale, "Scale");
                    ui.add_enabled(d.fit == Fit::Scale, egui::DragValue::new(&mut d.scale_percent).range(5.0..=1000.0).suffix(" %"));
                });
                ui.add_enabled(d.fit == Fit::Scale, egui::Checkbox::new(&mut d.auto_fit_if_too_large, "Auto-fit if scale too large"));
                ui.separator();
                ui.label("Colors");
                ui.checkbox(&mut d.invert, "Invert black and white");
                ui.checkbox(&mut d.black_and_white, "Black and white only");
                ui.checkbox(&mut d.draw_frame, "Draw frame");
                ui.separator();
                ui.label("Header and footer");
                for (label, text, just) in [("Header", &mut d.header, &mut d.header_justify), ("Footer", &mut d.footer, &mut d.footer_justify)] {
                    ui.horizontal(|ui| {
                        ui.label(label);
                        ui.add(egui::TextEdit::singleline(text).desired_width(150.0));
                        egui::ComboBox::from_id_salt(label).selected_text(format!("{just:?}")).width(70.0).show_ui(ui, |ui| {
                            for j in [Justify::Left, Justify::Center, Justify::Right] {
                                ui.selectable_value(just, j, format!("{j:?}"));
                            }
                        });
                    });
                }
                ui.horizontal(|ui| {
                    ui.label("Font size (pt)");
                    ui.add(egui::DragValue::new(&mut d.font_pt).range(6.0..=36.0));
                });
                ui.weak("Macros: %page%  %pages%  %file%");
            });
            ui.add_space(12.0); // a separator here would stretch to the full window height
            ui.vertical(|ui| {
                draw_page(ui, prev, d, 0, vec2(320.0, 420.0));
            });
        });
        ui.separator();
        ui.horizontal(|ui| {
            ok = ui.button("OK").clicked();
            cancel = ui.button("Cancel").clicked();
        });
    });
    if ok {
        let mut s = app.settings();
        s.page_setup = d.clone();
        app.apply_settings_quiet(s);
    }
    open && !ok && !cancel
}

pub fn print_preview(app: &mut App, ctx: &egui::Context, prev: &mut Preview, page: &mut usize) -> bool {
    let mut open = true;
    let (mut print, mut close) = (false, false);
    let setup = app.settings().page_setup;
    egui::Window::new("Print Preview").open(&mut open).collapsible(false).default_size([520.0, 680.0]).show(ctx, |ui| {
        let n = draw_page(ui, prev, &setup, *page, vec2(480.0, 600.0));
        *page = (*page).min(n - 1);
        ui.horizontal(|ui| {
            if ui.add_enabled(*page > 0, egui::Button::new("Prev")).clicked() {
                *page -= 1;
            }
            ui.label(format!("Page {} of {}", *page + 1, n));
            if ui.add_enabled(*page + 1 < n, egui::Button::new("Next")).clicked() {
                *page += 1;
            }
            ui.separator();
            print = ui.button("Print…").clicked();
            close = ui.button("Close").clicked();
        });
    });
    if print {
        app.print_document(false);
        return false;
    }
    open && !close
}
