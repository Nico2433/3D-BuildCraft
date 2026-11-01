//! "Print on paper": a light window in the style of the system's print dialog, with the
//! printers, page settings and a preview of the page.

use super::icons::Icon;
use super::style;
use super::*;
use crate::paper::{ColorMode, Job, MODEL};
use std::sync::mpsc::{channel, Receiver, TryRecvError};
use std::sync::Arc;

const SIZE: Vec2 = Vec2::new(566.0, 488.0);
const INK: Color32 = Color32::from_gray(0x1A);
const LINK: Color32 = Color32::from_rgb(0x00, 0x5F, 0xB8);
const BORDER: Color32 = Color32::from_gray(0xD6);
const SHADE: Color32 = Color32::from_gray(0xF3);
/// Left column: x and width.
const COL: (f32, f32) = (16.0, 178.0);
const FONT: f32 = 10.0;

enum Loaded {
    Printers(Vec<String>, Option<String>),
    Model(Vec<u8>),
}

pub(super) struct Dialog {
    title: String,
    /// None while they are being listed.
    printers: Option<Vec<String>>,
    printer: usize,
    landscape: bool,
    copies: u32,
    color: usize,
    allow: bool,
    /// The model picture, `MODEL`² RGBA.
    model: Option<Arc<Vec<u8>>>,
    /// The page preview and the (landscape, grey) it shows.
    preview: Option<((bool, bool), egui::TextureHandle)>,
    rx: Receiver<Loaded>,
    /// The job sent; the window is hidden until it is taken.
    printing: Option<Receiver<anyhow::Result<()>>>,
}

impl Dialog {
    /// Lists the printers and draws the model on their own threads.
    pub fn open(title: String, objects: &[Object], ctx: &egui::Context) -> Self {
        let (tx, rx) = channel();
        let parts: Vec<_> = objects
            .iter()
            .map(|o| (o.mesh.clone(), o.matrix(), o.color))
            .collect();
        let (list_tx, list_ctx) = (tx.clone(), ctx.clone());
        std::thread::spawn(move || {
            let (names, default) = crate::paper::printers().unwrap_or_else(|e| {
                log::warn!("could not list printers: {e:#}");
                Default::default()
            });
            let _ = list_tx.send(Loaded::Printers(names, default));
            list_ctx.request_repaint();
        });
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let meshes: Vec<Mesh> = parts.iter().map(|(m, t, _)| m.transformed(t)).collect();
            let refs: Vec<(&Mesh, Rgba)> =
                meshes.iter().zip(&parts).map(|(m, p)| (m, p.2)).collect();
            let _ = tx.send(Loaded::Model(crate::thumbnail::render_parts(&refs, MODEL)));
            ctx.request_repaint();
        });
        Self {
            title,
            printers: None,
            printer: 0,
            landscape: false,
            copies: 1,
            color: 0,
            allow: true,
            model: None,
            preview: None,
            rx,
            printing: None,
        }
    }

    /// The printers and the model picture are in.
    pub fn ready(&self) -> bool {
        self.printers.is_some() && self.model.is_some()
    }

    fn take_loaded(&mut self) {
        for msg in self.rx.try_iter() {
            match msg {
                Loaded::Printers(names, default) => {
                    self.printer = names
                        .iter()
                        .position(|n| Some(n) == default.as_ref())
                        .unwrap_or(0);
                    self.printers = Some(names);
                }
                Loaded::Model(px) => self.model = Some(Arc::new(px)),
            }
        }
    }

    /// Saves the page where the print system can read it and sends it on a thread.
    fn print(&mut self, ctx: &egui::Context) {
        let (Some(model), Some(printer)) = (
            self.model.clone(),
            self.printers.as_ref().and_then(|p| p.get(self.printer)),
        ) else {
            return;
        };
        let job = Job {
            printer: printer.clone(),
            copies: self.copies,
            landscape: self.landscape,
            color: ColorMode::ALL[self.color],
            title: self.title.clone(),
        };
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let dir = crate::slicer::print_dir();
            let path = dir.join("page.png");
            let page = crate::paper::page(&model, job.landscape, job.color == ColorMode::Gray);
            let sent = std::fs::create_dir_all(&dir)
                .map_err(anyhow::Error::from)
                .and_then(|()| page.save(&path).map_err(anyhow::Error::from))
                .and_then(|()| crate::paper::print(&path, &job));
            let _ = tx.send(sent);
            ctx.request_repaint();
        });
        self.printing = Some(rx);
    }

    /// The page preview, made again when its settings change.
    fn preview(&mut self, ctx: &egui::Context) -> Option<&egui::TextureHandle> {
        let key = (
            self.landscape,
            ColorMode::ALL[self.color] == ColorMode::Gray,
        );
        if self.preview.as_ref().is_none_or(|(k, _)| *k != key) {
            let page = crate::paper::page(self.model.as_ref()?, key.0, key.1);
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [page.width() as usize, page.height() as usize],
                page.as_raw(),
            );
            let tex = ctx.load_texture("paper page", image, egui::TextureOptions::LINEAR);
            self.preview = Some((key, tex));
        }
        self.preview.as_ref().map(|(_, t)| t)
    }

    /// Draws the window; returns true to close it.
    fn ui(&mut self, ui: &mut egui::Ui, lang: Lang) -> bool {
        let (rect, _) = ui.allocate_exact_size(SIZE, Sense::hover());
        *ui.visuals_mut() = egui::Visuals::light();
        let at = |x: f32, y: f32| rect.min + Vec2::new(x, y);
        let p = ui.painter().clone();
        let text = |pos: Pos2, t: &str, size: f32, color: Color32| {
            p.text(
                pos,
                Align2::LEFT_CENTER,
                t,
                egui::FontId::proportional(size),
                color,
            );
        };
        text(
            at(16.0, 23.0),
            &format!("{} - {}", self.title, tr(lang, "Printing")),
            FONT + 0.5,
            INK,
        );
        let x = egui::Rect::from_center_size(at(SIZE.x - 16.0, 11.0), Vec2::splat(22.0));
        let close = ui.interact(x, ui.id().with("close"), Sense::click());
        if close.hovered() {
            p.rect_filled(x, 4.0, SHADE);
        }
        super::icons::paint(&p, x.shrink(6.0), Icon::Close, INK);

        // Left column.
        let label = |y: f32, t: &str| text(at(COL.0, y), t, FONT, INK);
        label(59.0, tr(lang, "Printer"));
        let names: Vec<&str> = match &self.printers {
            Some(p) if p.is_empty() => vec![tr(lang, "No printers found")],
            Some(p) => p.iter().map(String::as_str).collect(),
            None => vec!["…"],
        };
        dropdown(
            ui,
            at(COL.0, 71.0),
            "paper_printer",
            &names,
            &mut self.printer,
            false,
        );
        let soon = |ui: &mut egui::Ui, y: f32, plus: bool, t: &str| {
            let color = LINK.gamma_multiply(0.55);
            let left = if plus {
                let c = at(COL.0 + 5.0, y);
                for d in [Vec2::X, Vec2::Y] {
                    p.line_segment([c - d * 4.5, c + d * 4.5], Stroke::new(1.0_f32, color));
                }
                COL.0 + 17.0
            } else {
                COL.0
            };
            let r = p.text(
                at(left, y),
                Align2::LEFT_CENTER,
                t,
                egui::FontId::proportional(FONT),
                color,
            );
            ui.interact(r, ui.id().with(t), Sense::hover())
                .on_hover_text(tr(lang, SOON));
        };
        soon(ui, 120.0, true, tr(lang, "Add a printer"));
        label(135.0, tr(lang, "Orientation"));
        let mut k = self.landscape as usize;
        let orient = [tr(lang, "Portrait"), tr(lang, "Landscape")];
        dropdown(
            ui,
            at(COL.0, 147.0),
            "paper_orientation",
            &orient,
            &mut k,
            true,
        );
        self.landscape = k == 1;
        label(190.0, tr(lang, "Copies"));
        self.copies_field(
            ui,
            egui::Rect::from_min_size(at(COL.0, 201.0), Vec2::new(COL.1, 22.0)),
        );
        label(241.0, tr(lang, "Color mode"));
        let modes = ColorMode::ALL.map(|m| tr(lang, m.label()));
        dropdown(
            ui,
            at(COL.0, 253.0),
            "paper_color",
            &modes,
            &mut self.color,
            false,
        );
        soon(ui, 295.0, false, tr(lang, "More settings"));

        // The page, its arrows and the fit button.
        let area = egui::Rect::from_min_max(at(210.0, 53.0), at(550.0, 417.0));
        p.rect(
            area,
            4.0,
            SHADE,
            Stroke::new(1.0_f32, BORDER),
            egui::StrokeKind::Inside,
        );
        let grey = Color32::from_gray(0x9A);
        for (cx, dir) in [(319.0, -1.0), (399.0, 1.0)] {
            let c = at(cx, 31.0);
            p.add(egui::Shape::convex_polygon(
                vec![
                    c + Vec2::new(2.0 * dir, 0.0),
                    c + Vec2::new(-2.0 * dir, -3.0),
                    c + Vec2::new(-2.0 * dir, 3.0),
                ],
                grey,
                Stroke::NONE,
            ));
        }
        p.text(
            at(359.0, 31.0),
            Align2::CENTER_CENTER,
            "1 / 1",
            egui::FontId::proportional(FONT),
            INK,
        );
        let fit = egui::Rect::from_center_size(at(443.0, 31.0), Vec2::new(11.0, 8.0));
        p.rect_stroke(
            fit,
            1.5,
            Stroke::new(1.0_f32, INK),
            egui::StrokeKind::Inside,
        );
        let title = self.title.clone();
        match self.preview(ui.ctx()) {
            Some(tex) => {
                let scale = ((area.size() - Vec2::splat(6.0)) / tex.size_vec2()).min_elem();
                let page = egui::Rect::from_center_size(area.center(), tex.size_vec2() * scale);
                p.image(
                    tex.id(),
                    page,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
                p.rect_stroke(
                    page,
                    0.0,
                    Stroke::new(1.0_f32, Color32::from_gray(0x80)),
                    egui::StrokeKind::Outside,
                );
                // shortcut: the name label shows in the preview only, as the page picture has
                // no text; draw it into the picture if it should print too.
                let tag = egui::Rect::from_min_size(
                    page.left_bottom() + Vec2::new(8.0, -20.0),
                    Vec2::new(46.0, 20.0),
                );
                p.rect_filled(tag, 0.0, Color32::from_gray(0x99));
                p.with_clip_rect(tag).text(
                    tag.left_top() + Vec2::new(3.0, 5.0),
                    Align2::LEFT_CENTER,
                    &title,
                    egui::FontId::proportional(4.0),
                    Color32::from_gray(0xE0),
                );
            }
            None => {
                ui.put(area, egui::Spinner::new());
            }
        }

        // Footer.
        let foot = egui::Rect::from_min_max(at(0.0, 434.0), rect.max);
        p.rect_filled(
            foot,
            egui::CornerRadius {
                sw: 8,
                se: 8,
                ..Default::default()
            },
            SHADE,
        );
        p.hline(foot.x_range(), foot.top(), Stroke::new(1.0_f32, BORDER));
        let mut c = child(
            ui,
            egui::Rect::from_min_max(at(16.0, 445.0), at(260.0, 478.0)),
        );
        c.style_mut().wrap_mode = Some(egui::TextWrapMode::Wrap);
        if self.allow {
            let w = &mut c.visuals_mut().widgets;
            for v in [&mut w.inactive, &mut w.hovered, &mut w.active] {
                (v.bg_fill, v.fg_stroke.color) = (LINK, Color32::WHITE);
            }
        }
        c.add(egui::Checkbox::new(
            &mut self.allow,
            egui::RichText::new(tr(lang, "Let the app change my print preferences"))
                .size(FONT)
                .color(INK),
        ));
        let can_print = self.ready() && self.printers.as_ref().is_some_and(|p| !p.is_empty());
        let button = |ui: &mut egui::Ui, x: f32, t: &str, enabled: bool| {
            let b = egui::Button::new(egui::RichText::new(t).size(FONT).color(INK))
                .fill(Color32::WHITE)
                .stroke(Stroke::new(1.0_f32, BORDER))
                .corner_radius(4.0);
            let r = egui::Rect::from_min_size(at(x, 451.0), Vec2::new(128.0, 22.0));
            child(ui, r)
                .add_enabled(enabled, b.min_size(r.size()))
                .clicked()
        };
        if button(ui, 289.0, tr(lang, "Print"), can_print) {
            self.print(ui.ctx());
        }
        let cancel = button(ui, 423.0, tr(lang, "Cancel"), true);
        close.clicked() || cancel
    }

    /// The number of copies, typed or stepped with the chevrons at the right.
    fn copies_field(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        field_box(ui, rect, false);
        let number = egui::Rect::from_min_max(
            rect.min + Vec2::new(6.0, 2.0),
            rect.max - Vec2::new(50.0, 2.0),
        );
        let mut c = child(ui, number);
        let w = &mut c.visuals_mut().widgets;
        w.inactive.weak_bg_fill = Color32::TRANSPARENT;
        w.inactive.bg_stroke = Stroke::NONE;
        w.hovered.weak_bg_fill = Color32::TRANSPARENT;
        c.add(egui::DragValue::new(&mut self.copies).range(1..=99));
        for (k, icon, step) in [(1.0, Icon::ChevronUp, 1i32), (0.0, Icon::ChevronDown, -1)] {
            let r = egui::Rect::from_center_size(
                rect.right_center() - Vec2::new(12.0 + 24.0 * k, 0.0),
                Vec2::new(22.0, rect.height() - 4.0),
            );
            let resp = ui.interact(r, ui.id().with(("copies", k as i32)), Sense::click());
            if resp.hovered() {
                ui.painter().rect_filled(r, 3.0, SHADE);
            }
            super::icons::paint(
                ui.painter(),
                egui::Rect::from_center_size(r.center(), Vec2::splat(9.0)),
                icon,
                INK,
            );
            if resp.clicked() {
                self.copies = self.copies.saturating_add_signed(step).clamp(1, 99);
            }
        }
    }
}

/// A child laid out left to right in `rect`, which takes no room in `ui`.
fn child(ui: &mut egui::Ui, rect: egui::Rect) -> egui::Ui {
    ui.new_child(
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(egui::Layout::left_to_right(egui::Align::Center)),
    )
}

/// A light field: white with a grey edge, darker when hovered.
fn field_box(ui: &egui::Ui, rect: egui::Rect, hovered: bool) {
    let edge = if hovered {
        Color32::from_gray(0x9A)
    } else {
        BORDER
    };
    ui.painter().rect(
        rect,
        4.0,
        Color32::WHITE,
        Stroke::new(1.0_f32, edge),
        egui::StrokeKind::Inside,
    );
}

/// A light drop-down in the left column, optionally with a page icon turned to the
/// orientation; the list that opens is the app's own ([`style::list`]).
fn dropdown(
    ui: &mut egui::Ui,
    min: Pos2,
    id: &str,
    options: &[&str],
    selected: &mut usize,
    page: bool,
) {
    let rect = egui::Rect::from_min_size(min, Vec2::new(COL.1, 25.0));
    let resp = ui.interact(rect, ui.id().with(id), Sense::click());
    field_box(ui, rect, resp.hovered());
    let p = ui.painter();
    let mut left = rect.left() + 10.0;
    if page {
        let size = if *selected == 1 {
            Vec2::new(11.0, 8.5)
        } else {
            Vec2::new(8.5, 11.0)
        };
        let icon = egui::Rect::from_center_size(egui::pos2(left + 6.0, rect.center().y), size);
        p.rect_stroke(
            icon,
            1.0,
            Stroke::new(1.0_f32, INK),
            egui::StrokeKind::Inside,
        );
        left += 25.0;
    }
    let current = options.get(*selected).copied().unwrap_or_default();
    let max = rect.right() - 26.0 - left;
    let galley = p.layout(
        current.to_owned(),
        egui::FontId::proportional(FONT),
        INK,
        max,
    );
    p.galley(
        egui::pos2(left, rect.center().y - galley.size().y / 2.0),
        galley,
        INK,
    );
    let chevron =
        egui::Rect::from_center_size(rect.right_center() - Vec2::new(14.0, 0.0), Vec2::splat(9.0));
    super::icons::paint(p, chevron, Icon::ChevronDown, INK);

    let open_id = ui.id().with((id, "open"));
    let mut open = ui.data(|d| d.get_temp::<bool>(open_id)).unwrap_or(false) != resp.clicked();
    if open && options.len() > 1 {
        if let Some(k) = style::list(
            ui.ctx(),
            open_id,
            rect.left_bottom(),
            COL.1,
            options,
            Some(*selected),
            &mut open,
            resp.clicked(),
        ) {
            *selected = k;
        }
    }
    ui.data_mut(|d| d.insert_temp(open_id, open));
}

impl App {
    /// The print-on-paper window while it is open, and the result of a job once sent.
    pub(super) fn paper_dialog(&mut self, ctx: &egui::Context) {
        let Some(d) = &mut self.paper else {
            return;
        };
        d.take_loaded();
        let lang = self.settings.lang();
        if let Some(rx) = &d.printing {
            let sent = match rx.try_recv() {
                Ok(r) => r,
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("the print job stopped")),
            };
            self.paper = None;
            match sent {
                Ok(()) => self.toast(tr(lang, "Sent to the printer."), false),
                Err(e) => self.toast(format!("{}: {e:#}", tr(lang, "Could not print")), true),
            }
            return;
        }
        let frame = egui::Frame::new()
            .fill(Color32::WHITE)
            .corner_radius(8.0)
            .stroke(Stroke::new(1.0_f32, BORDER))
            .shadow(ctx.style().visuals.popup_shadow);
        let modal = egui::Modal::new(egui::Id::new("paper"))
            .backdrop_color(Color32::TRANSPARENT)
            .frame(frame)
            .show(ctx, |ui| d.ui(ui, lang));
        let escape = modal.is_top_modal
            && !modal.any_popup_open
            && ctx.input(|i| i.key_pressed(Key::Escape));
        if modal.inner || escape {
            self.paper = None;
        }
    }
}
