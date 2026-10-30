//! Paint's colour menus, as the original's: the swatch list (eyedropper, more colours,
//! recent and standard colours) and "Add to custom colours" with hue, saturation and
//! brightness.

use super::icons::Icon;
use super::style;
use super::*;
use eframe::egui::ecolor::Hsva;

/// Which colour menu is open under Paint's "Selected" swatch.
#[derive(Clone, Default)]
pub(super) enum PaintMenu {
    #[default]
    Closed,
    Swatches,
    /// Hue, saturation and brightness (0 to 1) and the hex text being typed.
    More {
        hsv: [f32; 3],
        hex: String,
    },
}

/// The original's standard colours, in its order.
const STANDARD: [Rgba; 12] = [
    [0xFF, 0xFF, 0xFF, 0xFF],
    [0x00, 0x00, 0x00, 0xFF],
    [0x80, 0x80, 0x80, 0xFF],
    [0x88, 0x00, 0x15, 0xFF],
    [0xEC, 0x1B, 0x23, 0xFF],
    [0xFF, 0x7F, 0x25, 0xFF],
    [0xFE, 0xF1, 0x00, 0xFF],
    [0xB5, 0xE6, 0x1D, 0xFF],
    [0x21, 0xBB, 0x4C, 0xFF],
    [0x00, 0xA0, 0xE8, 0xFF],
    [0x3E, 0x47, 0xCB, 0xFF],
    [0xA3, 0x49, 0xA4, 0xFF],
];

/// Most recent colours kept.
pub(super) const RECENT: usize = 6;

const ROW_DARK: Color32 = Color32::from_rgb(0x1A, 0x1A, 0x1A);
const ROW: Color32 = Color32::from_rgb(0x2B, 0x2B, 0x2B);
const BUTTON: Color32 = Color32::from_rgb(0x55, 0x55, 0x55);
const KNOB: Color32 = Color32::from_rgb(0x88, 0x17, 0x98);

fn rgb(c: Rgba) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn hex(c: Rgba) -> String {
    format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn from_hsv(hsv: [f32; 3]) -> Rgba {
    let [r, g, b] = Hsva::new(hsv[0], hsv[1], hsv[2], 1.0).to_srgb();
    [r, g, b, 0xFF]
}

/// Paint's "Selected" swatch with its chevron, and whichever menu is open. True when the
/// eyedropper was chosen.
pub(super) fn selected_color(
    ui: &mut egui::Ui,
    lang: Lang,
    color: &mut Rgba,
    menu: &mut PaintMenu,
    recent: &[Rgba],
) -> bool {
    let label = ui.painter().layout_no_wrap(
        tr(lang, "Selected").to_owned(),
        egui::FontId::proportional(style::LABEL_FONT + 1.3),
        style::TEXT,
    );
    let w = label.size().x + 10.0;
    let (button, resp) = ui.allocate_exact_size(Vec2::new(w + 70.0, 36.0), Sense::click());
    if resp.hovered() {
        ui.painter()
            .rect_filled(button, egui::CornerRadius::ZERO, style::BLUE_HOVER);
    }
    let text_at = egui::pos2(
        button.left() + 4.0,
        button.center().y - label.size().y / 2.0,
    );
    ui.painter().galley(text_at, label, style::TEXT);
    // The swatch and chevron follow the label; the menus hang from this part.
    let rect = egui::Rect::from_min_size(
        egui::pos2(button.left() + w, button.center().y - 15.0),
        Vec2::new(64.0, 30.0),
    );
    let swatch = egui::Rect::from_min_size(rect.min + Vec2::new(0.0, 3.0), Vec2::splat(24.0));
    ui.painter()
        .rect_filled(swatch, egui::CornerRadius::ZERO, rgb(*color));
    let chevron = egui::Rect::from_min_size(rect.min + Vec2::new(36.0, 7.0), Vec2::splat(16.0));
    super::icons::paint(ui.painter(), chevron, Icon::ChevronDown, style::TEXT);
    let opening = resp.clicked();
    if opening {
        *menu = match menu {
            PaintMenu::Closed => PaintMenu::Swatches,
            _ => PaintMenu::Closed,
        };
    }
    let mut dropper = false;
    let ctx = ui.ctx().clone();
    let font = egui::FontId::proportional(13.0);
    match menu {
        PaintMenu::Closed => {}
        PaintMenu::Swatches => {
            let size = Vec2::new(337.0, 349.0);
            let mut next = None;
            let area = egui::Area::new(egui::Id::new("paint_swatches"))
                .fixed_pos(egui::pos2(rect.right() + 12.0, rect.top() + 18.0))
                .order(egui::Order::Foreground)
                .fade_in(false)
                .show(&ctx, |ui| {
                    let (panel, _) = ui.allocate_exact_size(size, Sense::hover());
                    let p = ui.painter().clone();
                    p.rect_filled(panel, egui::CornerRadius::ZERO, ROW_DARK);
                    let mut y = panel.top();
                    let mut band = |h: f32, fill: Color32| {
                        let r = egui::Rect::from_min_size(
                            egui::pos2(panel.left(), y),
                            Vec2::new(size.x, h),
                        );
                        p.rect_filled(r, egui::CornerRadius::ZERO, fill);
                        y += h;
                        r
                    };
                    let text = |r: egui::Rect, t: &str| {
                        p.text(
                            r.left_center() + Vec2::new(12.0, 0.0),
                            Align2::LEFT_CENTER,
                            t,
                            font.clone(),
                            style::TEXT,
                        )
                    };
                    for (k, label) in [tr(lang, "Eyedropper"), tr(lang, "More colors...")]
                        .into_iter()
                        .enumerate()
                    {
                        let r = band(48.0, BUTTON);
                        band(2.0, ROW);
                        let hit = ui.interact(r, ui.id().with(("paint_row", k)), Sense::click());
                        if hit.hovered() {
                            p.rect_filled(r, egui::CornerRadius::ZERO, Color32::from_gray(0x63));
                        }
                        text(r, label);
                        if hit.clicked() {
                            next = Some(k);
                        }
                    }
                    // Swatch rows: 20 point squares, 56 points apart.
                    let mut swatches = |colors: &[Rgba], top: f32, ui: &mut egui::Ui| {
                        for (k, c) in colors.iter().enumerate() {
                            let centre = egui::pos2(
                                panel.left() + 25.0 + (k % 6) as f32 * 56.0,
                                top + 28.0 + (k / 6) as f32 * 56.0,
                            );
                            let r = egui::Rect::from_center_size(centre, Vec2::splat(20.0));
                            let hit = ui.interact(
                                r.expand(6.0),
                                ui.id().with(("swatch", top as i32, k)),
                                Sense::click(),
                            );
                            if hit.hovered() {
                                p.rect_stroke(
                                    r.expand(3.0),
                                    egui::CornerRadius::ZERO,
                                    Stroke::new(1.0_f32, style::TEXT),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            p.rect_filled(r, egui::CornerRadius::ZERO, rgb(*c));
                            if hit.clicked() {
                                *color = *c;
                                next = Some(9);
                            }
                        }
                    };
                    let h = band(40.0, ROW_DARK);
                    text(h, tr(lang, "Recent colors"));
                    let r = band(56.0, ROW);
                    swatches(recent, r.top(), ui);
                    let h = band(40.0, ROW_DARK);
                    text(h, tr(lang, "Standard colors"));
                    let r = band(112.0, ROW);
                    swatches(&STANDARD, r.top(), ui);
                });
            match next {
                Some(0) => {
                    dropper = true;
                    *menu = PaintMenu::Closed;
                }
                Some(1) => {
                    let h = Hsva::from_srgb([color[0], color[1], color[2]]);
                    *menu = PaintMenu::More {
                        hsv: [h.h, h.s, h.v],
                        hex: hex(*color),
                    };
                }
                Some(_) => *menu = PaintMenu::Closed,
                None if !opening && area.response.clicked_elsewhere() => *menu = PaintMenu::Closed,
                None => {}
            }
        }
        PaintMenu::More { hsv, hex: text } => {
            let size = Vec2::new(308.0, 321.0);
            let mut accept = false;
            let area = egui::Area::new(egui::Id::new("paint_more"))
                .fixed_pos(egui::pos2(rect.left() - 90.0, rect.top() + 47.0))
                .order(egui::Order::Foreground)
                .fade_in(false)
                .show(&ctx, |ui| {
                    let (panel, _) = ui.allocate_exact_size(size, Sense::hover());
                    let p = ui.painter().clone();
                    p.rect_filled(panel, egui::CornerRadius::ZERO, Color32::from_gray(0x29));
                    let head = egui::Rect::from_min_size(panel.min, Vec2::new(size.x, 40.0));
                    p.rect_filled(head, egui::CornerRadius::ZERO, Color32::from_gray(0x19));
                    p.rect_stroke(
                        panel,
                        egui::CornerRadius::ZERO,
                        Stroke::new(1.0_f32, Color32::from_gray(0x76)),
                        egui::StrokeKind::Inside,
                    );
                    p.text(
                        head.left_center() + Vec2::new(13.0, 0.0),
                        Align2::LEFT_CENTER,
                        tr(lang, "Add to custom colors"),
                        egui::FontId::proportional(12.0),
                        style::TEXT,
                    );
                    let current = from_hsv(*hsv);
                    let sw = egui::Rect::from_min_size(
                        panel.min + Vec2::new(13.0, 69.0),
                        Vec2::splat(24.0),
                    );
                    p.rect_filled(sw, egui::CornerRadius::ZERO, rgb(current));
                    // The hex box: white with a blue edge, as the bottom bar's editor.
                    let input = egui::Rect::from_min_size(
                        panel.min + Vec2::new(49.0, 66.0),
                        Vec2::new(185.0, 30.0),
                    );
                    p.rect_filled(input, egui::CornerRadius::ZERO, Color32::WHITE);
                    p.rect_stroke(
                        input,
                        egui::CornerRadius::ZERO,
                        Stroke::new(2.0_f32, style::TOGGLE_ON),
                        egui::StrokeKind::Inside,
                    );
                    let mut child = ui.new_child(
                        egui::UiBuilder::new().max_rect(input.shrink2(Vec2::new(6.0, 4.0))),
                    );
                    let w = child.visuals_mut();
                    w.override_text_color = Some(Color32::BLACK);
                    w.extreme_bg_color = Color32::WHITE;
                    w.selection.bg_fill = style::TOGGLE_ON;
                    let r = child.add_sized(
                        Vec2::new(input.width() - 36.0, 22.0),
                        egui::TextEdit::singleline(text)
                            .frame(false)
                            .font(egui::FontId::proportional(style::MENU_FONT)),
                    );
                    if r.changed() {
                        if let Some(c) = parse_hex(text) {
                            let h = Hsva::from_srgb([c[0], c[1], c[2]]);
                            *hsv = [h.h, h.s, h.v];
                        }
                    }
                    let clear = egui::Rect::from_center_size(
                        input.right_center() - Vec2::new(14.0, 0.0),
                        Vec2::splat(18.0),
                    );
                    super::icons::paint(
                        &p,
                        clear.shrink(3.0),
                        Icon::Close,
                        Color32::from_gray(0x30),
                    );
                    if ui
                        .interact(clear, ui.id().with("hex_clear"), Sense::click())
                        .clicked()
                    {
                        text.clear();
                    }
                    let check = egui::Rect::from_min_size(
                        panel.min + Vec2::new(247.0, 65.0),
                        Vec2::new(48.0, 32.0),
                    );
                    let c = ui.interact(check, ui.id().with("hex_ok"), Sense::click());
                    p.rect_filled(
                        check,
                        egui::CornerRadius::ZERO,
                        if c.hovered() {
                            Color32::from_gray(0x66)
                        } else {
                            BUTTON
                        },
                    );
                    super::icons::paint(
                        &p,
                        check.shrink2(Vec2::new(14.0, 6.0)),
                        Icon::Check,
                        style::TEXT,
                    );
                    accept = c.clicked()
                        || (r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));
                    let track = |row: f32| {
                        egui::Rect::from_min_size(
                            panel.min + Vec2::new(13.0, row - 8.0),
                            Vec2::new(size.x - 26.0, 16.0),
                        )
                    };
                    let hue: Vec<Color32> = (0..=6)
                        .map(|k| rgb(from_hsv([k as f32 / 6.0, 1.0, 1.0])))
                        .collect();
                    let sat = [
                        rgb(from_hsv([hsv[0], 0.0, hsv[2]])),
                        rgb(from_hsv([hsv[0], 1.0, hsv[2]])),
                    ];
                    let bri = [Color32::BLACK, rgb(from_hsv([hsv[0], hsv[1], 1.0]))];
                    let mut moved = false;
                    for (k, (label, stops, row)) in [
                        (tr(lang, "Hue"), &hue[..], 163.0),
                        (tr(lang, "Saturation"), &sat[..], 229.0),
                        (tr(lang, "Brightness"), &bri[..], 296.0),
                    ]
                    .into_iter()
                    .enumerate()
                    {
                        p.text(
                            panel.min + Vec2::new(13.0, row - 32.0),
                            Align2::LEFT_CENTER,
                            label,
                            egui::FontId::proportional(14.0),
                            style::TEXT,
                        );
                        moved |= gradient_slider(ui, k, track(row), stops, &mut hsv[k]);
                    }
                    if moved {
                        *text = hex(from_hsv(*hsv));
                    }
                });
            if accept {
                *color = from_hsv(*hsv);
                *menu = PaintMenu::Closed;
            } else if !opening && area.response.clicked_elsewhere() {
                *menu = PaintMenu::Closed;
            }
        }
    }
    dropper
}

/// "DC4E62" or "#dc4e62" as a colour.
fn parse_hex(s: &str) -> Option<Rgba> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 {
        return None;
    }
    let v = u32::from_str_radix(s, 16).ok()?;
    Some([(v >> 16) as u8, (v >> 8) as u8, v as u8, 0xFF])
}

/// A thin track coloured through `stops` with the original's purple pill as the handle.
/// True when it moved.
fn gradient_slider(
    ui: &mut egui::Ui,
    k: usize,
    rect: egui::Rect,
    stops: &[Color32],
    v: &mut f32,
) -> bool {
    let resp = ui.interact(rect, ui.id().with(("gradient", k)), Sense::click_and_drag());
    let before = *v;
    if let Some(p) = resp.interact_pointer_pos() {
        *v = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    }
    let y = rect.center().y;
    let mut mesh = egui::Mesh::default();
    let n = stops.len() - 1;
    for (i, c) in stops.iter().enumerate() {
        let x = rect.left() + rect.width() * i as f32 / n as f32;
        mesh.colored_vertex(egui::pos2(x, y - 1.0), *c);
        mesh.colored_vertex(egui::pos2(x, y + 1.0), *c);
        if i > 0 {
            let b = (2 * i) as u32;
            mesh.add_triangle(b - 2, b - 1, b);
            mesh.add_triangle(b - 1, b + 1, b);
        }
    }
    ui.painter().add(mesh);
    let knob = egui::Rect::from_center_size(
        egui::pos2(rect.left() + *v * rect.width(), y),
        Vec2::new(24.0, 8.0),
    );
    ui.painter()
        .rect_filled(knob, egui::CornerRadius::same(4), KNOB);
    *v != before
}

impl App {
    /// Paints the connected region around triangle `tri` of object `id`.
    pub(super) fn paint_area(&mut self, id: u64, tri: usize, color: Rgba, coverage: f32) {
        self.remember_color(color);
        self.edit(|s| {
            if let Some(o) = s.get_mut(id) {
                o.mesh = std::sync::Arc::new(paint_region(&o.mesh, tri, color, coverage));
            }
        });
    }

    /// Puts `c` first in Paint's recent colours.
    pub(super) fn remember_color(&mut self, c: Rgba) {
        let recent = &mut self.settings.recent_colors;
        recent.retain(|r| *r != c);
        recent.insert(0, c);
        recent.truncate(paint_menu::RECENT);
        let _ = self.settings.save();
    }
}

fn paint_region(mesh: &Mesh, tri: usize, color: Rgba, coverage: f32) -> Mesh {
    let mut mesh = mesh.clone();
    let area = mesh.face_region(tri, coverage.clamp(0.0, 1.0) * 180.0);
    mesh.colors.resize(mesh.triangles.len(), [0; 4]);
    for face in area {
        mesh.colors[face] = color;
    }
    mesh
}

#[cfg(test)]
mod tests {
    #[test]
    fn zero_coverage_paints_only_the_connected_flat_region() {
        let mut cube = crate::primitives::Primitive::Cube.build();
        let old_color = [0, 0, 255, 255];
        let new_color = [255, 0, 0, 255];
        cube.colors = vec![old_color; cube.triangles.len()];
        let painted = super::paint_region(&cube, 0, new_color, 0.0);
        assert_eq!(
            painted.colors.iter().filter(|&&c| c == new_color).count(),
            2
        );
        assert_eq!(
            painted.colors.iter().filter(|&&c| c == old_color).count(),
            10
        );
        assert!(cube.colors.iter().all(|&c| c == old_color));
    }

    #[test]
    fn full_coverage_paints_connected_primitives() {
        for primitive in [
            crate::primitives::Primitive::Sphere,
            crate::primitives::Primitive::Cylinder,
        ] {
            let mesh = primitive.build();
            let color = [255, 0, 0, 255];
            let painted = super::paint_region(&mesh, 0, color, 1.0);
            assert!(painted.colors.iter().all(|&c| c == color));
        }
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(super::parse_hex("#DC4E62"), Some([0xDC, 0x4E, 0x62, 0xFF]));
        assert_eq!(super::parse_hex("dc4e6"), None);
        assert_eq!(super::hex([0xDC, 0x4E, 0x62, 0xFF]), "DC4E62");
    }
}
