//! The Add dialog (Insert › Add): recent files as tiles, load an object or an image, and
//! the 3D library: folders the user adds as libraries.

use super::icons::Icon;
use super::style;
use super::*;
use crate::library::{self, Entry};

const DIALOG: Color32 = Color32::from_rgb(0x1F, 0x1F, 0x1F);
const TILE: Color32 = Color32::from_rgb(0x13, 0x13, 0x13);
const LIB_BORDER: Color32 = Color32::from_rgb(0x1F, 0x6E, 0x8C);

/// A folder (none: the list of libraries) and a search text.
type ListKey = (Option<PathBuf>, String);

/// Where the library panel is: the list of libraries, or a folder inside one, and the
/// search text; with the entries found there, read again only when this changes.
#[derive(Default)]
pub struct LibraryView {
    pub folder: Option<PathBuf>,
    pub query: String,
    listed: Option<(Option<PathBuf>, String, Vec<Entry>)>,
    /// A listing being read on a thread, for this folder and search.
    pending: Option<(ListKey, std::sync::mpsc::Receiver<Vec<Entry>>)>,
}

impl LibraryView {
    /// Forget what was read, so the next look reads the folders again.
    pub fn refresh(&mut self) {
        self.listed = None;
        self.pending = None;
    }
}

impl App {
    pub(super) fn add_dialog(&mut self, ctx: &egui::Context) {
        if !self.add_open {
            return;
        }
        let lang = self.settings.lang();
        let screen = ctx.screen_rect();
        // Dim the window behind; a click there closes the dialog.
        egui::Area::new(egui::Id::new("add_dim"))
            .fixed_pos(screen.min)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                let r = ui.allocate_rect(screen, Sense::click());
                ui.painter().rect_filled(
                    screen,
                    egui::CornerRadius::ZERO,
                    Color32::from_black_alpha(90),
                );
                if r.clicked() {
                    self.add_open = false;
                }
            });
        let size = Vec2::new(950.0, 557.0).min(screen.size() - Vec2::splat(40.0));
        let rect = egui::Rect::from_center_size(screen.center() + Vec2::new(0.0, 20.0), size);
        egui::Area::new(egui::Id::new("add_dialog"))
            .fixed_pos(rect.min)
            .order(egui::Order::Tooltip)
            .show(ctx, |ui| {
                ui.set_min_size(rect.size());
                ui.set_max_size(rect.size());
                ui.painter()
                    .rect_filled(rect, egui::CornerRadius::ZERO, DIALOG);
                let close = egui::Rect::from_min_size(
                    rect.right_top() + Vec2::new(-52.0, 12.0),
                    Vec2::splat(40.0),
                );
                if ui.put(close, egui::Button::new("").frame(false)).clicked() {
                    self.add_open = false;
                }
                super::icons::paint(ui.painter(), close.shrink(10.0), Icon::Close, style::TEXT);

                // Left column: title, recent files, load buttons.
                let left = egui::Rect::from_min_size(
                    rect.min + Vec2::new(14.0, 8.0),
                    Vec2::new(196.0, size.y - 16.0),
                );
                let mut col = ui.new_child(egui::UiBuilder::new().max_rect(left));
                col.label(
                    egui::RichText::new(tr(lang, "Add"))
                        .size(32.0)
                        .color(style::TEXT),
                );
                col.add_space(8.0);
                col.label(
                    egui::RichText::new(tr(lang, "Recent"))
                        .size(style::MENU_FONT)
                        .strong()
                        .color(style::TEXT),
                );
                col.add_space(8.0);
                let recent: Vec<_> = self.settings.recent.iter().take(6).cloned().collect();
                egui::Grid::new("add_recent")
                    .spacing(Vec2::splat(6.0))
                    .show(&mut col, |ui| {
                        for (i, r) in recent.iter().enumerate() {
                            let thumb = self.thumbs.file(&r.path);
                            let resp = model_tile(ui, 92.0, thumb, None);
                            if resp.on_hover_text(r.path.display().to_string()).clicked() {
                                self.actions.push(Action::AddPath(r.path.clone()));
                                self.add_open = false;
                            }
                            if i % 2 == 1 {
                                ui.end_row();
                            }
                        }
                    });
                if recent.is_empty() {
                    col.label(
                        egui::RichText::new(tr(lang, "No recent files")).color(style::TEXT_DIM),
                    );
                }
                let rest = left.bottom() - col.cursor().top() - 3.0 * 40.0 - 14.0;
                col.add_space(rest.max(8.0));
                let (sep, _) = col.allocate_exact_size(Vec2::new(196.0, 1.0), Sense::hover());
                col.painter()
                    .rect_filled(sep, egui::CornerRadius::ZERO, Color32::from_gray(0x79));
                col.add_space(8.0);
                for (icon, label, action) in [
                    (Icon::Open, tr(lang, "Load object"), Some(Action::AddFile)),
                    (
                        Icon::Texture,
                        tr(lang, "Load image"),
                        Some(Action::StartImage),
                    ),
                    (Icon::Eye, tr(lang, "Camera"), Some(Action::StartCamera)),
                ] {
                    let enabled = action.is_some();
                    let bar = style::Bar {
                        height: 40.0,
                        hover: style::RIBBON_HOVER,
                        active: style::ACTIVE,
                    };
                    let r = style::ribbon_button(&mut col, bar, icon, label, enabled, false);
                    if r.clicked() {
                        self.actions.extend(action.clone());
                        self.add_open = false;
                    }
                    if !enabled {
                        r.on_hover_text(tr(lang, SOON));
                    }
                }

                // Right: the 3D library.
                let lib = egui::Rect::from_min_max(
                    egui::pos2(left.right() + 16.0, rect.top() + 48.0),
                    rect.right_bottom() - Vec2::new(14.0, 14.0),
                );
                ui.painter()
                    .rect_filled(lib, egui::CornerRadius::ZERO, Color32::BLACK);
                ui.painter().rect_stroke(
                    lib,
                    egui::CornerRadius::ZERO,
                    Stroke::new(1.5_f32, LIB_BORDER),
                    egui::StrokeKind::Inside,
                );
                let mut panel = ui.new_child(egui::UiBuilder::new().max_rect(lib.shrink(12.0)));
                self.library_panel(&mut panel);
            });
    }

    /// The saved library folders and any given with `--library` (not saved).
    fn libraries(&self) -> Vec<PathBuf> {
        self.settings
            .libraries
            .iter()
            .chain(&self.extra_libraries)
            .cloned()
            .collect()
    }

    fn library_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang();
        ui.visuals_mut().override_text_color = Some(style::TEXT);
        // Header: back (to the parent folder or the list of libraries), title, add folder.
        ui.horizontal(|ui| {
            let can_back = self.library.folder.is_some();
            if style::icon_button(
                ui,
                Icon::Back,
                Vec2::splat(32.0),
                can_back,
                style::RIBBON_HOVER,
            )
            .clicked()
            {
                let folder = self.library.folder.take().unwrap();
                let root = self.libraries().contains(&folder);
                self.library.folder = if root {
                    None
                } else {
                    folder.parent().map(Path::to_path_buf)
                };
            }
            let title = match &self.library.folder {
                Some(f) => f
                    .file_name()
                    .map_or(String::new(), |n| n.to_string_lossy().into_owned()),
                None => tr(lang, "3D library").to_owned(),
            };
            ui.label(egui::RichText::new(title).size(22.0).color(style::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let add = ui.add(
                    egui::Button::new(
                        egui::RichText::new(tr(lang, "Add library folder")).color(style::TEXT),
                    )
                    .fill(Color32::from_gray(0x2A))
                    .corner_radius(0.0),
                );
                if add.clicked() {
                    if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                        if !self.settings.libraries.contains(&dir) {
                            self.settings.libraries.push(dir);
                            let _ = self.settings.save();
                        }
                        self.library.refresh();
                    }
                }
            });
        });
        ui.add_space(8.0);
        let search = egui::TextEdit::singleline(&mut self.library.query)
            .hint_text(tr(lang, "Search"))
            .desired_width(f32::INFINITY)
            .font(egui::FontId::proportional(style::MENU_FONT));
        ui.add(search);
        ui.add_space(10.0);

        if self.libraries().is_empty() {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "Add a folder with 3D models to use it as a library.",
                    ))
                    .color(style::TEXT_DIM)
                    .size(style::MENU_FONT),
                );
            });
            return;
        }
        // Read the folder (or search) again only when where we are changes.
        let key = (
            self.library.folder.clone(),
            self.library.query.trim().to_owned(),
        );
        let libraries: Vec<PathBuf> = self.libraries();
        // Reading folders can be slow (big trees, network drives): it runs on a thread and
        // the panel shows the result when it arrives.
        let stale = self
            .library
            .listed
            .as_ref()
            .is_none_or(|(f, q, _)| (f, q) != (&key.0, &key.1));
        let asked = self
            .library
            .pending
            .as_ref()
            .is_some_and(|(k, _)| *k == key);
        if stale && !asked {
            self.thumbs.new_page();
            let (tx, rx) = std::sync::mpsc::channel();
            let (folder, query) = key.clone();
            std::thread::spawn(move || {
                let entries = match (&folder, query.is_empty()) {
                    (Some(f), true) => library::list(f),
                    (Some(f), false) => library::search(f, &query),
                    (None, true) => libraries
                        .iter()
                        .map(|p| Entry {
                            path: p.clone(),
                            name: p.file_name().map_or_else(
                                || p.display().to_string(),
                                |n| n.to_string_lossy().into_owned(),
                            ),
                            is_dir: true,
                        })
                        .collect(),
                    (None, false) => libraries
                        .iter()
                        .flat_map(|l| library::search(l, &query))
                        .take(library::MAX_RESULTS)
                        .collect(),
                };
                let _ = tx.send(entries);
            });
            self.library.pending = Some((key.clone(), rx));
        }
        if let Some((k, rx)) = &self.library.pending {
            if let Ok(entries) = rx.try_recv() {
                self.library.listed = Some((k.0.clone(), k.1.clone(), entries));
                self.library.pending = None;
            } else {
                ui.ctx().request_repaint_after(Duration::from_millis(100));
            }
        }
        if stale && self.library.pending.is_some() {
            ui.label(egui::RichText::new("…").size(20.0).color(style::TEXT_DIM));
            return;
        }
        let entries = self
            .library
            .listed
            .as_ref()
            .map(|(_, _, e)| e.clone())
            .unwrap_or_default();
        let top = self.library.folder.is_none() && self.library.query.trim().is_empty();
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            // Folders as wide category bars, like the original's, then models as tiles.
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::new(14.0, 12.0);
                for e in entries.iter().filter(|e| e.is_dir) {
                    let (r, resp) = ui.allocate_exact_size(Vec2::new(300.0, 64.0), Sense::click());
                    let fill = if resp.hovered() {
                        style::RIBBON_HOVER
                    } else {
                        DIALOG
                    };
                    ui.painter().rect_filled(r, egui::CornerRadius::ZERO, fill);
                    ui.painter().text(
                        r.left_center() + Vec2::new(12.0, 0.0),
                        Align2::LEFT_CENTER,
                        &e.name,
                        egui::FontId::proportional(style::MENU_FONT),
                        style::TEXT,
                    );
                    super::icons::paint(
                        ui.painter(),
                        egui::Rect::from_center_size(
                            r.right_center() - Vec2::new(24.0, 0.0),
                            Vec2::splat(22.0),
                        ),
                        Icon::ChevronRight,
                        style::TEXT,
                    );
                    let resp = resp.on_hover_text(e.path.display().to_string());
                    if top {
                        resp.context_menu(|ui| {
                            if ui.button(tr(lang, "Remove library")).clicked() {
                                remove = Some(e.path.clone());
                                ui.close_menu();
                            }
                        });
                    }
                    if resp.clicked() {
                        self.library.folder = Some(e.path.clone());
                        self.library.query.clear();
                    }
                }
            });
            ui.add_space(12.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(10.0);
                for e in entries.iter().filter(|e| !e.is_dir) {
                    let thumb = self.thumbs.file(&e.path);
                    let resp = model_tile(ui, 110.0, thumb, Some(&e.name));
                    if resp.on_hover_text(e.path.display().to_string()).clicked() {
                        self.actions.push(Action::AddPath(e.path.clone()));
                        self.add_open = false;
                    }
                }
            });
            if entries.is_empty() {
                ui.label(egui::RichText::new(tr(lang, "No models here.")).color(style::TEXT_DIM));
            }
            if top {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(tr(lang, "Right-click a library to remove it."))
                        .size(style::LABEL_FONT)
                        .color(style::TEXT_DIM),
                );
            }
        });
        if let Some(p) = remove {
            self.settings.libraries.retain(|l| *l != p);
            self.extra_libraries.retain(|l| *l != p);
            let _ = self.settings.save();
            self.library.refresh();
        }
    }
}

/// A square tile with a model's thumbnail (a cube while it is being made) and, below it,
/// an optional name.
fn model_tile(
    ui: &mut egui::Ui,
    side: f32,
    thumb: Option<egui::TextureId>,
    name: Option<&str>,
) -> egui::Response {
    let height = side + if name.is_some() { 30.0 } else { 0.0 };
    let (t, resp) = ui.allocate_exact_size(Vec2::new(side, height), Sense::click());
    let image = egui::Rect::from_min_size(t.min, Vec2::splat(side));
    let fill = if resp.hovered() {
        style::RIBBON_HOVER
    } else {
        TILE
    };
    ui.painter()
        .rect_filled(image, egui::CornerRadius::ZERO, fill);
    match thumb {
        Some(id) => {
            ui.painter().image(
                id,
                image.shrink(6.0),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => super::icons::paint(
            ui.painter(),
            image.shrink(side * 0.3),
            Icon::Cube,
            Color32::from_gray(0x60),
        ),
    }
    if let Some(name) = name {
        let galley = ui.painter().layout(
            name.to_owned(),
            egui::FontId::proportional(11.0),
            style::TEXT,
            side,
        );
        ui.painter().galley(
            egui::pos2(t.left(), image.bottom() + 4.0),
            galley,
            style::TEXT,
        );
    }
    resp
}
