//! Telling the user about a new version: the check at startup, the bar under the start
//! screen's tiles, the question it asks, the download card and the restart.

use super::icons::Icon;
use super::style;
use super::*;
use crate::update::{self, install, PackageKind, Release, Version};
use std::sync::{mpsc, Arc};

/// Text on the download card.
const DOWNLOADING: &str = "Downloading the update...";
/// The bar under the start screen's tiles.
pub(super) const BAR_H: f32 = 44.0;

/// What is known about updates this session.
pub(super) struct UpdateState {
    /// The release check under way.
    check: Option<mpsc::Receiver<Result<Option<Release>, String>>>,
    /// Checked already this session, or never to be (screenshots).
    checked: bool,
    /// A newer release, once found.
    pub available: Option<Release>,
    /// How this copy was installed.
    kind: PackageKind,
    /// The "update now?" question is open.
    pub dialog: bool,
}

impl UpdateState {
    /// `offline` (screenshot runs) never checks; `fake` pretends that version was found.
    pub fn new(offline: bool, fake: Option<Version>) -> Self {
        if !offline {
            install::remove_previous();
        }
        Self {
            check: None,
            checked: offline || fake.is_some(),
            available: fake.map(Release::placeholder),
            kind: PackageKind::detect(),
            dialog: false,
        }
    }
}

impl App {
    /// Starts the check once, if the settings allow it, and takes its answer.
    pub(super) fn update_step(&mut self, ctx: &egui::Context) {
        let u = &mut self.update;
        if !u.checked && self.settings.check_updates {
            u.checked = true;
            let (tx, rx) = mpsc::channel();
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let found = update::fetch::check(Version::current()).map_err(|e| format!("{e:#}"));
                let _ = tx.send(found);
                ctx.request_repaint();
            });
            u.check = Some(rx);
        }
        let Some(answer) = u.check.as_ref().and_then(|rx| rx.try_recv().ok()) else {
            return;
        };
        u.check = None;
        match answer {
            Ok(found) => u.available = found,
            // A failed check is not worth interrupting anyone for.
            Err(e) => log::info!("update check failed: {e}"),
        }
    }

    /// The bar along the bottom of the start screen's panel (`rect`), when a newer
    /// version is out; a click opens the question.
    pub(super) fn update_bar(&mut self, ui: &mut egui::Ui, rect: egui::Rect) {
        let Some(release) = &self.update.available else {
            return;
        };
        let lang = self.settings.lang();
        let resp = ui.allocate_rect(rect, Sense::click());
        let p = ui.painter();
        // Coloured as the tiles above it, lighter under the pointer like them.
        let fill = if resp.hovered() {
            super::start::TILE_HOVER
        } else {
            super::start::TILE
        };
        p.rect_filled(rect, egui::CornerRadius::ZERO, fill);
        let icon = egui::Rect::from_center_size(
            rect.left_center() + Vec2::new(15.0 + 14.0, 0.0),
            Vec2::splat(26.0),
        );
        super::icons::paint(p, icon, Icon::Download, style::TEXT);
        let text = tr(lang, "Update available: version {to}. Click to update.")
            .replace("{to}", &release.version.to_string());
        p.text(
            egui::pos2(icon.right() + 12.0, rect.center().y),
            Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(style::MENU_FONT),
            style::TEXT,
        );
        if resp.clicked() {
            self.update.dialog = true;
        }
    }

    /// The question the bar asks: update now (or open the download page when this copy
    /// cannot update itself).
    pub(super) fn update_dialog(&mut self, ctx: &egui::Context) {
        let Some(release) = self.update.available.clone().filter(|_| self.update.dialog) else {
            return;
        };
        let lang = self.settings.lang();
        let (from, to) = (Version::current().to_string(), release.version.to_string());
        let (question, go) = match self.update.kind {
            PackageKind::Other => (
                tr(
                    lang,
                    "Version {to} of 3D BuildCraft is available. This copy cannot update itself: open the download page?",
                ),
                tr(lang, "Open page"),
            ),
            PackageKind::Msi => (
                tr(
                    lang,
                    "Update 3D BuildCraft from version {from} to {to}? The installer will open and the app will close.",
                ),
                tr(lang, "Update"),
            ),
            _ => (
                tr(
                    lang,
                    "Update 3D BuildCraft from version {from} to {to}? The app will restart.",
                ),
                tr(lang, "Update"),
            ),
        };
        let mut text = question.replace("{from}", &from).replace("{to}", &to);
        if self.scene.dirty && self.update.kind.installs() {
            text.push(' ');
            text.push_str(tr(
                lang,
                "There are unsaved changes: you can save them before it restarts.",
            ));
        }
        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let buttons = [go, tr(lang, "Cancel")];
        match super::dialogs::black_box(ctx, "update", &text, 560.0, &buttons, false) {
            Some(0) => {
                self.update.dialog = false;
                if self.update.kind.installs() {
                    self.start_update(&release, ctx);
                } else {
                    self.actions.push(Action::OpenReleasePage(release.page));
                }
            }
            Some(_) => self.update.dialog = false,
            None if escape => self.update.dialog = false,
            None => {}
        }
    }

    /// Downloads and installs on a job thread behind a progress card.
    fn start_update(&mut self, release: &Release, ctx: &egui::Context) {
        let (kind, release) = (self.update.kind.clone(), release.clone());
        let steps = update::fetch::PROGRESS_STEPS;
        self.jobs
            .push(jobs::Job::spawn(DOWNLOADING, steps, ctx, move |done| {
                let page = release.page.clone();
                let result = install::prepare(&kind, &release, done).map_err(|e| format!("{e:#}"));
                jobs::Done::Updated { result, page }
            }));
    }

    /// The update is in place: restart (after the unsaved-changes question, if any); or
    /// say why it failed and offer the download page.
    pub(super) fn update_done(&mut self, result: Result<install::Restart, String>, page: String) {
        let lang = self.settings.lang();
        match result {
            Ok(restart) => self
                .actions
                .push(Action::RestartForUpdate(Arc::new(restart))),
            Err(e) => {
                let text = format!(
                    "{} {e}. {}",
                    tr(lang, "Could not update:"),
                    tr(lang, "Click here to open the download page.")
                );
                self.toast_action(text, Action::OpenReleasePage(page));
            }
        }
    }

    /// Starts the new version and closes this one.
    pub(super) fn restart_for_update(&mut self, restart: &install::Restart, ctx: &egui::Context) {
        let lang = self.settings.lang();
        match install::restart(restart) {
            Ok(()) => {
                self.scene.dirty = false;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            Err(e) => self.toast(
                format!(
                    "{} {e:#}. {}",
                    tr(lang, "Could not restart:"),
                    tr(
                        lang,
                        "The new version opens the next time you start the app."
                    )
                ),
                true,
            ),
        }
    }

    /// Opens a release page (GitHub only) in the browser.
    pub(super) fn open_release_page(&mut self, page: &str) {
        if let Err(e) = update::open_page(page) {
            let lang = self.settings.lang();
            self.toast(
                format!("{} {e:#}", tr(lang, "Could not open the page:")),
                true,
            );
        }
    }
}
