//! Windows: recovery, settings, about, save prompt, and the notices at the bottom right.

use super::icons::Icon;
use super::style;
use super::*;

impl App {
    pub(super) fn dialogs(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang();
        if let Some(path) = self.restore.first().cloned() {
            let text = tr(
                lang,
                "Unsaved changes from the previous session were found. Do you want to load them again?",
            );
            let buttons = [tr(lang, "Continue"), tr(lang, "Ignore")];
            match black_box(ctx, "restore", text, 505.0, &buttons, true) {
                Some(0) => self.actions.push(Action::Restore(path)),
                Some(_) => self.actions.push(Action::DiscardRestore),
                None => {}
            }
        }

        if self.about_open {
            let mut open = true;
            egui::Window::new(tr(lang, "About 3D BuildCraft"))
                .id(egui::Id::new("about"))
                .open(&mut open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!(
                        "{} {}",
                        tr(lang, "Version"),
                        env!("CARGO_PKG_VERSION")
                    ));
                    ui.label(tr(
                        lang,
                        "Build, repair and prepare 3D models for printing.",
                    ));
                    ui.label(tr(lang, "Windows, Linux and macOS."));
                });
            self.about_open = open;
        }

        if self.confirm.is_some() {
            let buttons = [
                tr(lang, "Save scene"),
                tr(lang, "Discard scene"),
                tr(lang, "Cancel"),
            ];
            let text = tr(lang, "Unsaved changes will be lost.");
            let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            match black_box(ctx, "confirm", text, 370.0, &buttons, false) {
                Some(0) => self.save(false),
                Some(1) => self.scene.dirty = false,
                Some(_) => self.confirm = None,
                None if escape => self.confirm = None,
                None => {}
            }
            // Saved or discarded: run what was waiting.
            if !self.scene.dirty {
                self.actions.extend(self.confirm.take());
            }
        }

        // Notices: dark cards at the bottom right with a coloured edge.
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        let mut cards: Vec<(Icon, String, Color32, Option<Action>)> = Vec::new();
        let repairing = self.jobs.iter().any(|j| j.label == super::REPAIRING);
        if !self.invalid.is_empty()
            && self.pending_import.is_empty()
            && self.tool.is_none()
            && !repairing
        {
            cards.push((
                Icon::Wrench,
                tr(
                    lang,
                    "One or more objects are not valid. Click here to repair them.",
                )
                .to_owned(),
                style::CARD_ERROR,
                Some(Action::Repair),
            ));
        }
        if let Some(
            Tool::Emboss {
                preview: Err(e), ..
            }
            | Tool::Custom {
                preview: Err(e), ..
            }
            | Tool::Image {
                preview: Err(e), ..
            },
        ) = &self.tool
        {
            if !e.is_empty() {
                cards.push((Icon::Info, e.clone(), style::CARD_INFO, None));
            }
        }
        if let Some(Tool::Camera { cam, .. }) = &self.tool {
            if let Some(Err(_)) = cam.latest() {
                let text = tr(lang, "No camera was found, or it is in use by another app.");
                cards.push((Icon::Info, text.to_owned(), style::CARD_INFO, None));
            }
        }
        if let Some(Tool::Paint { .. }) = self.tool {
            cards.push((
                Icon::Info,
                tr(
                    lang,
                    "To paint, pick a colour in Selected and click the object. Coverage sets how much of the surface around the click is painted.",
                )
                .to_owned(),
                style::CARD_INFO,
                None,
            ));
        }
        for t in &self.toasts {
            let edge = if t.error {
                style::CARD_ERROR
            } else {
                style::CARD_INFO
            };
            let icon = if t.error { Icon::Info } else { Icon::Check };
            cards.push((icon, t.text.clone(), edge, None));
        }
        if !self.toasts.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
        if cards.is_empty() && self.jobs.iter().all(|j| j.hidden) {
            return;
        }
        egui::Area::new(egui::Id::new("notices"))
            .anchor(Align2::RIGHT_BOTTOM, [0.0, 0.0])
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 4.0;
                // Running work first, with its progress bar and a close button.
                for job in self.jobs.iter_mut().filter(|j| !j.hidden) {
                    let text = tr(lang, job.label);
                    let (_, closed) = card(
                        ui,
                        Icon::FileUp,
                        text,
                        style::CARD_INFO,
                        false,
                        Some(job.fraction()),
                    );
                    job.hidden = closed;
                }
                for (icon, text, edge, action) in cards {
                    if card(ui, icon, &text, edge, action.is_some(), None)
                        .0
                        .clicked()
                    {
                        self.actions.extend(action);
                    }
                }
            });
    }
}

/// A notice: icon and wrapped text on a dark card, coloured stripe on the left. With
/// `progress` it also has a bar along the bottom and a close button; true when closed.
fn card(
    ui: &mut egui::Ui,
    icon: Icon,
    text: &str,
    edge: Color32,
    clickable: bool,
    progress: Option<f32>,
) -> (egui::Response, bool) {
    let width = 358.0;
    let galley = ui.painter().layout(
        text.to_owned(),
        egui::FontId::proportional(style::MENU_FONT),
        style::TEXT,
        width - 80.0,
    );
    let height = (galley.size().y + 32.0).max(76.0);
    let sense = if clickable {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(width, height), sense);
    let p = ui.painter();
    let fill = if clickable && resp.hovered() {
        style::RIBBON_HOVER
    } else {
        style::CARD
    };
    p.rect_filled(rect, egui::CornerRadius::ZERO, fill);
    p.rect_filled(
        egui::Rect::from_min_size(rect.min, Vec2::new(4.0, height)),
        egui::CornerRadius::ZERO,
        edge,
    );
    super::icons::paint(
        p,
        egui::Rect::from_center_size(
            egui::pos2(rect.left() + 32.0, rect.top() + 36.0),
            Vec2::splat(28.0),
        ),
        icon,
        style::TEXT,
    );
    p.galley(
        egui::pos2(rect.left() + 60.0, rect.top() + 16.0),
        galley,
        style::TEXT,
    );
    let Some(f) = progress else {
        return (resp, false);
    };
    // Colours from the original's "Repairing..." card.
    let track = egui::Rect::from_min_max(
        egui::pos2(rect.left() + 4.0, rect.bottom() - 4.0),
        rect.right_bottom(),
    );
    p.rect_filled(track, egui::CornerRadius::ZERO, Color32::from_gray(0x56));
    let mut bar = track;
    bar.set_width(track.width() * f.clamp(0.0, 1.0));
    p.rect_filled(
        bar,
        egui::CornerRadius::ZERO,
        Color32::from_rgb(0x01, 0x5E, 0x86),
    );
    let x = egui::Rect::from_center_size(
        egui::pos2(rect.right() - 20.0, rect.top() + 18.0),
        Vec2::splat(24.0),
    );
    let close = ui.interact(x, resp.id.with("close"), Sense::click());
    super::icons::paint(p, x.shrink(4.0), Icon::Close, style::TEXT);
    (resp, close.clicked())
}

/// The original's question box: the window dims and a black box shows `text` with grey
/// buttons from its bottom left, 24 px in from the edges; `fill` stretches them to share
/// the row. Returns the button clicked.
fn black_box(
    ctx: &egui::Context,
    id: &str,
    text: &str,
    width: f32,
    buttons: &[&str],
    fill: bool,
) -> Option<usize> {
    let screen = ctx.screen_rect();
    egui::Area::new(egui::Id::new((id, "dim")))
        .fixed_pos(screen.min)
        .fade_in(false)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            ui.painter().rect_filled(
                screen,
                egui::CornerRadius::ZERO,
                Color32::from_black_alpha(110),
            );
            // Clicks behind the box go nowhere.
            ui.allocate_rect(screen, Sense::click_and_drag());
        });
    let size = Vec2::new(width, 182.0);
    let rect = egui::Rect::from_center_size(screen.center(), size);
    let mut clicked = None;
    egui::Area::new(egui::Id::new(id))
        .fixed_pos(rect.min)
        .fade_in(false)
        .order(egui::Order::Tooltip)
        .show(ctx, |ui| {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::ZERO, Color32::BLACK);
            ui.set_min_size(size);
            let body = egui::Rect::from_min_size(
                rect.min + Vec2::new(24.0, 22.0),
                Vec2::new(size.x - 48.0, 60.0),
            );
            ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(body)
                    .layout(egui::Layout::top_down(egui::Align::Min)),
            )
            .add(
                egui::Label::new(egui::RichText::new(text).size(13.0).color(Color32::WHITE)).wrap(),
            );
            let row = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 24.0, rect.bottom() - 56.0),
                rect.right_bottom() - Vec2::new(24.0, 24.0),
            );
            let mut ui = ui.new_child(
                egui::UiBuilder::new()
                    .max_rect(row)
                    .layout(egui::Layout::left_to_right(egui::Align::Center)),
            );
            let gap = 4.0;
            ui.spacing_mut().item_spacing.x = gap;
            ui.spacing_mut().button_padding = Vec2::new(10.0, 5.0);
            let n = buttons.len() as f32;
            let w = if fill {
                (row.width() - gap * (n - 1.0)) / n
            } else {
                0.0
            };
            for (k, t) in buttons.iter().enumerate() {
                let b = egui::Button::new(egui::RichText::new(*t).size(13.0).color(Color32::WHITE))
                    .fill(Color32::from_gray(0x33))
                    .stroke(Stroke::NONE)
                    .corner_radius(0.0)
                    .min_size(Vec2::new(w, 32.0));
                if ui.add(b).clicked() {
                    clicked = Some(k);
                }
            }
        });
    clicked
}
