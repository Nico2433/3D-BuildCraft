//! Work that takes a while (loading a file, repairing) runs on its own thread while a
//! notice card shows it with a progress bar, as in the original.

use crate::io::LoadedModel;
use crate::mesh::Mesh;
use eframe::egui;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{mpsc, Arc};
use std::time::Instant;

/// What a finished job hands back to the app.
pub(super) enum Done {
    /// New meshes for objects, with the mesh each one had when the job started.
    Repaired(Vec<(u64, Arc<Mesh>, Mesh)>),
    Loaded {
        path: PathBuf,
        replace_scene: bool,
        model: anyhow::Result<LoadedModel>,
        /// Where on the floor it lands.
        at: glam::Vec3,
    },
}

pub(super) struct Job {
    /// Text for the card, an i18n key such as "Repairing...".
    pub label: &'static str,
    /// The card was closed; the job keeps running.
    pub hidden: bool,
    started: Instant,
    /// Steps done out of `steps`; with no steps the bar fills with time instead.
    done: Arc<AtomicU32>,
    steps: u32,
    rx: mpsc::Receiver<Done>,
}

impl Job {
    /// Runs `work` on a thread; it counts finished steps in the counter it gets.
    pub fn spawn(
        label: &'static str,
        steps: u32,
        ctx: &egui::Context,
        work: impl FnOnce(&AtomicU32) -> Done + Send + 'static,
    ) -> Job {
        let (tx, rx) = mpsc::channel();
        let done = Arc::new(AtomicU32::new(0));
        let (counter, ctx) = (done.clone(), ctx.clone());
        std::thread::spawn(move || {
            let _ = tx.send(work(&counter));
            ctx.request_repaint();
        });
        Job {
            label,
            hidden: false,
            started: Instant::now(),
            done,
            steps,
            rx,
        }
    }

    /// How full the bar is, 0 to 1.
    pub fn fraction(&self) -> f32 {
        if self.steps > 0 {
            return self.done.load(Ordering::Relaxed) as f32 / self.steps as f32;
        }
        // shortcut: no real progress from the loaders, so it eases towards 90 %; upgrade
        // when io::load can report how far it got.
        let t = self.started.elapsed().as_secs_f32();
        0.9 * (1.0 - (-t / 1.5).exp())
    }

    /// None while it runs; then the result, or None inside if the thread died.
    pub fn try_take(&self) -> Option<Option<Done>> {
        match self.rx.try_recv() {
            Ok(d) => Some(Some(d)),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(None),
        }
    }

    /// Waits for the result (for the agent, which answers only when the work is done).
    pub fn wait(self) -> Option<Done> {
        self.rx.recv().ok()
    }
}
