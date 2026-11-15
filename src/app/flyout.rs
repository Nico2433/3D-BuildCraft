//! The original's dark flyout menus, as Paint's colours, the Texture tool's pictures and
//! Emboss's patterns have them: full-width grey button rows (lighter under the pointer,
//! 2 point separators), dark section header bands and grids of tiles, stacked top to
//! bottom in an area that closes when the pointer clicks elsewhere.

use super::style;
use super::*;

/// The menu's background and its header bands.
pub(super) const ROW_DARK: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
/// Separators between button rows and the band behind colour swatches.
pub(super) const ROW: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2B);
/// A button row.
pub(super) const BUTTON: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
/// A button row under the pointer.
const BUTTON_HOVER: Color32 = Color32::from_gray(0x63);
/// The text of rows and headers.
const FONT: f32 = 13.0;
/// Where row and header text starts, from the menu's left.
const TEXT_INSET: f32 = 12.0;

/// Where a flyout opens and how its rows look; [`Flyout::show`] fills it.
pub(super) struct Flyout {
    id: egui::Id,
    at: Pos2,
    width: f32,
    row_h: f32,
    header_h: f32,
}

/// What [`Flyout::show`] gave back: what the contents returned, and whether a click
/// elsewhere asked to close it.
pub(super) struct Shown<R> {
    pub inner: R,
    pub dismissed: bool,
}

impl Flyout {
    /// A flyout `width` points wide with its top left at `at`, moved to stay inside the
    /// window. Button rows are 48 points high and headers 40.
    pub(super) fn new(id: impl std::hash::Hash, at: Pos2, width: f32) -> Self {
        Self {
            id: egui::Id::new(id),
            at,
            width,
            row_h: 48.0,
            header_h: 40.0,
        }
    }

    /// Other heights for its button rows and headers.
    pub(super) fn heights(mut self, row: f32, header: f32) -> Self {
        self.row_h = row;
        self.header_h = header;
        self
    }

    /// Shows the flyout with `add` filling it from the top. `opening` is true on the frame
    /// the button that opens it was clicked, so that click does not count as one
    /// elsewhere.
    pub(super) fn show<R>(
        self,
        ctx: &egui::Context,
        opening: bool,
        add: impl FnOnce(&mut Menu) -> R,
    ) -> Shown<R> {
        let area = egui::Area::new(self.id)
            .fixed_pos(self.at)
            .order(egui::Order::Foreground)
            .fade_in(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::ZERO;
                // The background goes behind whatever the rows paint.
                let background = ui.painter().add(egui::Shape::Noop);
                let mut menu = Menu {
                    ui,
                    width: self.width,
                    row_h: self.row_h,
                    header_h: self.header_h,
                    grids: 0,
                };
                let inner = add(&mut menu);
                let rect = menu.ui.min_rect();
                menu.ui.painter().set(
                    background,
                    egui::Shape::rect_filled(rect, egui::CornerRadius::ZERO, ROW_DARK),
                );
                inner
            });
        Shown {
            dismissed: !opening && area.response.clicked_elsewhere(),
            inner: area.inner,
        }
    }
}

/// The contents of an open flyout, added from the top down.
pub(super) struct Menu<'a> {
    /// For contents of a menu's own (a text field, a switch); they should take its width.
    pub ui: &'a mut egui::Ui,
    width: f32,
    row_h: f32,
    header_h: f32,
    /// Tile grids added so far, to tell their tiles apart.
    grids: usize,
}

/// How a grid lays out its square tiles and marks the one under the pointer.
pub(super) struct Grid {
    pub columns: usize,
    /// Each tile's side.
    pub tile: f32,
    /// From one tile's centre to the next, across and down.
    pub pitch: f32,
    /// The first tile's centre, from the grid band's top left.
    pub first: Vec2,
    /// The band is this much taller than its rows' pitch.
    pub extra: f32,
    pub fill: Color32,
    pub mark: Mark,
}

/// How a tile shows it is under the pointer or chosen.
#[derive(Clone, Copy, PartialEq)]
pub(super) enum Mark {
    /// A small colour square: a thin box round it under the pointer, a thick blue one a
    /// little further out when chosen, and a click target larger than the square.
    Swatch,
    /// A picture: a box just inside it, white under the pointer and blue when chosen.
    Picture,
}

impl Menu<'_> {
    /// A full-width band `height` points high in `fill`, for contents of a menu's own.
    pub(super) fn band(&mut self, height: f32, fill: Color32) -> egui::Rect {
        let (r, _) = self
            .ui
            .allocate_exact_size(Vec2::new(self.width, height), Sense::hover());
        self.ui
            .painter()
            .rect_filled(r, egui::CornerRadius::ZERO, fill);
        r
    }

    /// A grey button row with its label, then a separator. A disabled one is dimmed and
    /// does not react. True when clicked.
    pub(super) fn button_row(&mut self, label: &str, enabled: bool) -> bool {
        let h = self.row_h;
        self.row(label, enabled, h, TEXT_INSET, |_, _, _| {})
    }

    fn row(
        &mut self,
        label: &str,
        enabled: bool,
        height: f32,
        text_at: f32,
        icon: impl FnOnce(&egui::Painter, egui::Rect, Color32),
    ) -> bool {
        let sense = if enabled {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (r, resp) = self
            .ui
            .allocate_exact_size(Vec2::new(self.width, height), sense);
        let fill = if enabled && resp.hovered() {
            BUTTON_HOVER
        } else {
            BUTTON
        };
        let p = self.ui.painter();
        p.rect_filled(r, egui::CornerRadius::ZERO, fill);
        let color = if enabled {
            style::TEXT
        } else {
            style::TEXT_DIM
        };
        icon(p, r, color);
        p.text(
            r.left_center() + Vec2::new(text_at, 0.0),
            Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(FONT),
            color,
        );
        self.band(2.0, ROW);
        resp.clicked()
    }

    /// A dark section header with its label.
    pub(super) fn header(&mut self, label: &str) {
        let h = self.header_h;
        let r = self.band(h, ROW_DARK);
        self.ui.painter().text(
            r.left_center() + Vec2::new(TEXT_INSET, 0.0),
            Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(FONT),
            style::TEXT,
        );
    }

    /// A band of `count` tiles in rows of `grid.columns`, each drawn by `paint` (given its
    /// index and square) and marked when `selected` says so. The index of a tile clicked.
    pub(super) fn tile_grid(
        &mut self,
        count: usize,
        grid: &Grid,
        selected: impl Fn(usize) -> bool,
        mut paint: impl FnMut(&egui::Painter, usize, egui::Rect),
    ) -> Option<usize> {
        let rows = count.div_ceil(grid.columns.max(1));
        let band = self.band(rows as f32 * grid.pitch + grid.extra, grid.fill);
        self.grids += 1;
        let mut picked = None;
        for k in 0..count {
            let centre = band.min
                + grid.first
                + Vec2::new(
                    (k % grid.columns) as f32 * grid.pitch,
                    (k / grid.columns) as f32 * grid.pitch,
                );
            let r = egui::Rect::from_center_size(centre, Vec2::splat(grid.tile));
            let target = match grid.mark {
                Mark::Swatch => r.expand(6.0),
                Mark::Picture => r,
            };
            let id = self.ui.id().with(("flyout_tile", self.grids, k));
            let resp = self.ui.interact(target, id, Sense::click());
            let p = self.ui.painter();
            paint(p, k, r);
            let on = selected(k);
            let stroke = |r: egui::Rect, width: f32, color: Color32| {
                p.rect_stroke(
                    r,
                    egui::CornerRadius::ZERO,
                    Stroke::new(width, color),
                    egui::StrokeKind::Inside,
                );
            };
            match grid.mark {
                Mark::Swatch => {
                    if resp.hovered() {
                        stroke(r.expand(3.0), 1.0, style::TEXT);
                    }
                    if on {
                        stroke(r.expand(5.0), 2.0, style::TOGGLE_ON);
                    }
                }
                Mark::Picture => {
                    if resp.hovered() || on {
                        stroke(r, 2.0, if on { style::TOGGLE_ON } else { style::TEXT });
                    }
                }
            }
            if resp.clicked() {
                picked = Some(k);
            }
        }
        picked
    }
}
