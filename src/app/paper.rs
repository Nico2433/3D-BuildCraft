//! "Print on paper": draws the model and hands the page to the system's print dialog on a
//! thread, then tells how it went.

use super::*;
use crate::paper::{Outcome, MODEL};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

#[derive(Default)]
pub(super) struct Paper {
    /// The app window's X11 id, for the print dialog to sit on it.
    window: Option<u64>,
    /// The print dialog being shown.
    job: Option<Receiver<anyhow::Result<Outcome>>>,
}

impl App {
    /// Draws the model and shows the print dialog for it, unless one is already open.
    pub(super) fn print_on_paper(&mut self, ctx: &egui::Context) {
        if self.paper.job.is_some() {
            return;
        }
        let title = self.scene_title();
        let window = self.paper.window;
        let parts: Vec<_> = self
            .scene
            .objects
            .iter()
            .map(|o| (o.mesh.clone(), o.matrix(), o.color))
            .collect();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let meshes: Vec<Mesh> = parts.iter().map(|(m, t, _)| m.transformed(t)).collect();
            let refs: Vec<(&Mesh, Rgba)> =
                meshes.iter().zip(&parts).map(|(m, p)| (m, p.2)).collect();
            let model = crate::thumbnail::render_parts(&refs, MODEL);
            let _ = tx.send(crate::paper::print(&model, &title, window));
            ctx.request_repaint();
        });
        self.paper.job = Some(rx);
    }

    /// Notes the app window for the dialog and tells how a finished print went.
    pub(super) fn paper_job(&mut self, frame: &eframe::Frame) {
        if self.paper.window.is_none() {
            self.paper.window = x11_window(frame);
        }
        let Some(rx) = &self.paper.job else {
            return;
        };
        let done = match rx.try_recv() {
            Ok(r) => r,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err(anyhow::anyhow!("the print job stopped")),
        };
        self.paper.job = None;
        let lang = self.settings.lang();
        match done {
            Ok(Outcome::Printed) => self.toast(tr(lang, "Sent to the printer."), false),
            Ok(Outcome::Cancelled) => self.toast(tr(lang, "Printing cancelled."), false),
            Ok(Outcome::Shown) => {}
            Ok(Outcome::Opened) => self.toast(
                tr(
                    lang,
                    "No print dialog available: print the page from the viewer.",
                ),
                false,
            ),
            Err(e) => self.toast(format!("{}: {e:#}", tr(lang, "Could not print")), true),
        }
    }
}

#[cfg(target_os = "linux")]
fn x11_window(frame: &eframe::Frame) -> Option<u64> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match frame.window_handle().ok()?.as_raw() {
        RawWindowHandle::Xlib(h) => Some(h.window),
        RawWindowHandle::Xcb(h) => Some(h.window.get() as u64),
        _ => None,
    }
}

#[cfg(not(target_os = "linux"))]
fn x11_window(_: &eframe::Frame) -> Option<u64> {
    None
}
