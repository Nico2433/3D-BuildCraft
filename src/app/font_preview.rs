//! Installed fonts loaded into egui on demand, so Emboss's Font list can show each one's
//! name followed by "abc" in that font, as the original's. Only fonts whose rows are on
//! screen are loaded, a few per frame and up to a budget; they stay for the session.

use super::style;
use super::*;
use eframe::egui::{FontData, FontFamily, FontId};
use eframe::epaint::text::{FontInsert, FontPriority, InsertFontFamily};
use std::collections::HashSet;

/// The sample written after each font's name.
const SAMPLE: &str = "abc";
/// Fonts loaded at most and their data in bytes; past either, rows show the name alone.
const MAX_FONTS: usize = 96;
const MAX_BYTES: usize = 96 << 20;
/// Larger font files (big CJK collections) are not loaded for a sample.
const MAX_FILE: usize = 12 << 20;
/// Fonts loaded in one frame; the others follow in the next ones.
const PER_FRAME: usize = 6;
/// Space between a font's name and its sample.
const SAMPLE_GAP: f32 = 8.0;

/// The fonts asked for, and those egui can draw with.
#[derive(Default)]
pub(super) struct FontPreviews {
    /// Every family asked for (`None` is the built-in font), loaded or not.
    asked: HashSet<Option<String>>,
    /// Fonts loaded and their bytes, against the budget.
    loaded: usize,
    bytes: usize,
    /// Families egui has as of this frame, by their sample family name.
    ready: HashSet<String>,
}

/// The egui family a font's sample is drawn with.
fn family_name(font: Option<&str>) -> String {
    format!("emboss-sample:{}", font.unwrap_or(""))
}

impl FontPreviews {
    /// Notes which samples egui can draw this frame; call before painting rows.
    pub(super) fn begin(&mut self, ctx: &egui::Context) {
        if self.loaded == self.ready.len() {
            return;
        }
        self.ready = ctx.fonts(|f| {
            f.families()
                .into_iter()
                .filter_map(|fam| match fam {
                    FontFamily::Name(n) if n.starts_with("emboss-sample:") => Some(n.to_string()),
                    _ => None,
                })
                .collect()
        });
    }

    /// The width a row needs for `font`'s name and its sample.
    pub(super) fn row_width(ui: &egui::Ui, name: &str) -> f32 {
        let w = |t: &str| {
            ui.painter()
                .layout_no_wrap(
                    t.to_owned(),
                    FontId::proportional(style::MENU_FONT),
                    style::TEXT,
                )
                .size()
                .x
        };
        w(name) + SAMPLE_GAP + w(SAMPLE) * 1.4
    }

    /// Paints `name` at the left of `rect` and, when `font` is loaded, "abc" in it after
    /// the name. A row on screen whose font is not loaded yet is added to `wanted`.
    pub(super) fn paint_row(
        &self,
        p: &egui::Painter,
        rect: egui::Rect,
        name: &str,
        font: Option<&str>,
        wanted: &std::cell::RefCell<Vec<Option<String>>>,
    ) {
        let at = rect.left_center() + Vec2::new(10.0, 0.0);
        let font_id = FontId::proportional(style::MENU_FONT);
        let shown = p.text(at, Align2::LEFT_CENTER, name, font_id, style::TEXT);
        let family = family_name(font);
        if self.ready.contains(&family) {
            let at = egui::pos2(shown.right() + SAMPLE_GAP, at.y);
            let id = FontId::new(style::MENU_FONT, FontFamily::Name(family.into()));
            p.text(at, Align2::LEFT_CENTER, SAMPLE, id, style::TEXT);
        } else if p.clip_rect().intersects(rect) {
            wanted.borrow_mut().push(font.map(str::to_owned));
        }
    }

    /// Loads a few of the `wanted` fonts into egui (they draw from the next frame on),
    /// within the budget, and asks for another frame while some are left.
    pub(super) fn load(&mut self, ctx: &egui::Context, wanted: Vec<Option<String>>) {
        let mut fresh: Vec<Option<String>> = wanted
            .into_iter()
            .filter(|f| !self.asked.contains(f))
            .collect();
        fresh.dedup();
        let later = fresh.len() > PER_FRAME;
        for font in fresh.into_iter().take(PER_FRAME) {
            if !self.asked.insert(font.clone()) {
                continue;
            }
            if self.loaded >= MAX_FONTS || self.bytes >= MAX_BYTES {
                continue;
            }
            let data = match &font {
                None => Some((epaint_default_fonts::UBUNTU_LIGHT.to_vec(), 0)),
                Some(f) => crate::emboss::font_data(f),
            };
            // egui stops at a font it cannot read, so each is checked first.
            let readable = |(b, i): &(Vec<u8>, u32)| {
                b.len() <= MAX_FILE && ttf_parser::Face::parse(b, *i).is_ok()
            };
            let Some((bytes, index)) = data.filter(readable) else {
                continue;
            };
            self.loaded += 1;
            self.bytes += bytes.len();
            let name = family_name(font.as_deref());
            let mut data = FontData::from_owned(bytes);
            data.index = index;
            let family = InsertFontFamily {
                family: FontFamily::Name(name.clone().into()),
                priority: FontPriority::Highest,
            };
            ctx.add_font(FontInsert::new(&name, data, vec![family]));
        }
        if later || self.loaded > self.ready.len() {
            ctx.request_repaint();
        }
    }
}
