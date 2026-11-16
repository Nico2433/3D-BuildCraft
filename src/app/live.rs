//! Live previews of tools that change objects (Smooth, Extrude outline): the work runs on a
//! thread once the settings have rested for a moment, and only the latest answer is shown.

use std::time::{Duration, Instant};

/// How long the settings must stay put before the preview is rebuilt.
const DEBOUNCE: Duration = Duration::from_millis(150);

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
