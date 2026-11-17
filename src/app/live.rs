//! Live previews of tools that change objects (Smooth, Simplify, Extrude outline): the work
//! runs on a thread once the settings have rested for a moment, and only the latest answer
//! is shown. Apply keeps the preview as one undo step; Cancel rolls back to `before`.

use super::{App, Checkpoint, Tool};
use crate::mesh::Mesh;
use crate::scene::{Object, Scene};
use std::time::{Duration, Instant};

/// How long the settings must stay put before the preview is rebuilt.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// From this many triangles a preview shows its progress card: big models take seconds,
/// as in the original, while a card for every small change would only flicker.
pub(super) const CARD_FROM: usize = 200_000;

/// What a previewing tool has computed so far, for settings of type `S`.
pub(super) struct Run<S> {
    /// The objects the tool works on, fixed when it starts.
    pub ids: Vec<u64>,
    /// The settings the scene shows, the ones being computed and the latest ones.
    shown: S,
    running: Option<S>,
    target: S,
    /// When `target` last changed.
    since: Instant,
    /// Started jobs: an older one's answer is dropped.
    started: u64,
    /// The scene shows a result, not a failure.
    ok: bool,
}

impl<S: Copy + PartialEq> Run<S> {
    /// A run whose scene already shows `shown`.
    pub fn new(ids: Vec<u64>, shown: S) -> Self {
        Self {
            ids,
            shown,
            running: None,
            target: shown,
            since: Instant::now(),
            started: 0,
            ok: true,
        }
    }

    /// The scene shows these settings, and a failure counts as shown.
    pub fn is_shown(&self, s: S) -> bool {
        self.shown == s && self.running.is_none()
    }

    /// Apply can take the preview: it shows `s` and did not fail.
    pub fn ready(&self, s: S) -> bool {
        self.ok && self.is_shown(s)
    }

    /// The job to start for the `latest` settings, with its number, once they have rested;
    /// None while they are shown, being computed, or still moving.
    pub fn next(&mut self, latest: S, ctx: &eframe::egui::Context) -> Option<(u64, S)> {
        if latest != self.target {
            self.target = latest;
            self.since = Instant::now();
        }
        if self.target == self.shown || Some(self.target) == self.running {
            return None;
        }
        let wait = DEBOUNCE.saturating_sub(self.since.elapsed());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return None;
        }
        self.started += 1;
        self.running = Some(self.target);
        Some((self.started, self.target))
    }

    /// The settings `next` gave were shown without a job.
    pub fn shown_now(&mut self) {
        (self.shown, self.running, self.ok) = (self.target, None, true);
    }

    /// Job `gen` finished; false when a newer one has started, whose answer counts instead.
    pub fn finish(&mut self, gen: u64, ok: bool) -> bool {
        if gen != self.started {
            return false;
        }
        self.shown = self.running.take().unwrap_or(self.shown);
        self.ok = ok;
        true
    }
}

impl Tool {
    /// For a tool previewed live: whether the scene shows the settings now on its bar, and
    /// whether Apply can take that preview (it did not fail).
    fn live(&self) -> Option<(bool, bool)> {
        fn state<S: Copy + PartialEq>(run: &Run<S>, s: S) -> Option<(bool, bool)> {
            Some((run.is_shown(s), run.ready(s)))
        }
        match self {
            Tool::Smooth {
                amount, edges, run, ..
            } => state(run, (*amount, *edges)),
            Tool::Simplify { level, run, .. } => state(run, *level),
            Tool::Outline { opts, run, .. } => state(run, *opts),
            _ => None,
        }
    }
}

impl App {
    /// A live preview that is still waiting to start or to finish.
    pub(super) fn live_pending(&self) -> bool {
        let live = self.tool.as_ref().and_then(Tool::live);
        live.is_some_and(|(shown, _)| !shown)
    }

    /// Whether Apply can take the live preview: it shows the settings now on the bar.
    pub(super) fn live_ready(&self) -> bool {
        let live = self.tool.as_ref().and_then(Tool::live);
        live.is_some_and(|(_, ready)| ready)
    }

    /// Keeps the live preview as one undo step, once it shows the settings on the bar.
    pub(super) fn apply_live(&mut self) {
        if !self.live_ready() {
            return;
        }
        if let Some(
            Tool::Smooth { before, .. }
            | Tool::Simplify { before, .. }
            | Tool::Outline { before, .. },
        ) = self.tool.take()
        {
            self.commit(*before);
            self.scene.dirty = true;
            self.refresh_invalid();
        }
    }
}

/// The objects `ids` as they were before the tool.
pub(super) fn originals<'a>(
    before: &'a Checkpoint,
    ids: &'a [u64],
) -> impl Iterator<Item = &'a Object> + 'a {
    before.0.objects.iter().filter(|o| ids.contains(&o.id))
}

/// Puts object `id` back as it was before the tool, dropping its last preview, and
/// returns it for the next one to change.
fn restore<'a>(scene: &'a mut Scene, before: &Checkpoint, id: u64) -> Option<&'a mut Object> {
    let original = before.0.objects.iter().find(|o| o.id == id)?;
    let slot = scene.get_mut(id)?;
    *slot = original.clone();
    Some(slot)
}

/// Shows the objects `ids` as they were before the tool.
pub(super) fn restore_all(scene: &mut Scene, before: &Checkpoint, ids: &[u64]) {
    for &id in ids {
        restore(scene, before, id);
    }
}

/// Shows a finished mesh preview: each object as it was before the tool, given its new
/// mesh by `place`.
pub(super) fn show_meshes(
    scene: &mut Scene,
    before: &Checkpoint,
    meshes: Vec<(u64, Mesh)>,
    place: impl Fn(&mut Object, Mesh),
) {
    for (id, mesh) in meshes {
        if let Some(o) = restore(scene, before, id) {
            place(o, mesh);
        }
    }
}
