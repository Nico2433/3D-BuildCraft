//! Edit → Smooth: the scene shows the smoothed meshes while the slider moves. The work runs
//! on a thread once the settings have rested for a moment.

use super::*;

/// How long the settings must stay put before the preview is rebuilt.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// What the Smooth tool has done so far.
pub(super) struct Run {
    ids: Vec<u64>,
    /// The settings the scene shows, the ones being computed and the latest ones.
    shown: (f32, bool),
    running: Option<(f32, bool)>,
    target: (f32, bool),
    /// When `target` last changed.
    since: Instant,
    /// Started jobs: an older one's answer is dropped.
    started: u64,
    /// The scene shows a result, not a failure.
    ok: bool,
}

impl Run {
    /// The scene shows these settings, and a failure counts as shown.
    pub fn is_shown(&self, amount: f32, edges: bool) -> bool {
        self.shown == (amount, edges) && self.running.is_none()
    }

    pub fn new(ids: Vec<u64>, amount: f32, edges: bool) -> Self {
        Self {
            ids,
            shown: (amount, edges),
            running: None,
            target: (amount, edges),
            since: Instant::now(),
            started: 0,
            ok: true,
        }
    }
}

impl App {
    /// Starts the preview of the latest settings once they have rested.
    pub(super) fn smooth_step(&mut self, ctx: &egui::Context) {
        let Some(Tool::Smooth {
            amount,
            edges,
            before,
            run,
        }) = &mut self.tool
        else {
            return;
        };
        if (*amount, *edges) != run.target {
            run.target = (*amount, *edges);
            run.since = Instant::now();
        }
        if run.target == run.shown || Some(run.target) == run.running {
            return;
        }
        let wait = DEBOUNCE.saturating_sub(run.since.elapsed());
        if !wait.is_zero() {
            ctx.request_repaint_after(wait);
            return;
        }
        run.started += 1;
        let (amount, edges) = run.target;
        let originals: Vec<&Object> = before
            .0
            .objects
            .iter()
            .filter(|o| run.ids.contains(&o.id))
            .collect();
        if amount <= 0.0 {
            for o in originals {
                if let Some(slot) = self.scene.get_mut(o.id) {
                    *slot = o.clone();
                }
            }
            (run.shown, run.running, run.ok) = (run.target, None, true);
            return;
        }
        let work: Vec<_> = originals
            .iter()
            .map(|o| (o.id, o.mesh.clone(), o.matrix()))
            .collect();
        let gen = run.started;
        run.running = Some(run.target);
        let mut job = jobs::Job::spawn(SMOOTHING, 0, ctx, move |_| jobs::Done::Smoothed {
            gen,
            result: work
                .into_iter()
                .map(|(id, mesh, m)| {
                    crate::solid::smooth_by(&mesh.transformed(&m), amount, edges)
                        .map(|smooth| (id, smooth))
                })
                .collect::<anyhow::Result<_>>()
                .map_err(|e| format!("{e:#}")),
        });
        // A card for every slider stop would only flicker.
        job.hidden = true;
        self.jobs.push(job);
    }

    /// A finished preview: shown unless the settings moved on.
    pub(super) fn smooth_done(&mut self, gen: u64, result: Result<Vec<(u64, Mesh)>, String>) {
        let Some(Tool::Smooth { before, run, .. }) = &mut self.tool else {
            return;
        };
        if gen != run.started {
            return;
        }
        run.shown = run.running.take().unwrap_or(run.shown);
        run.ok = result.is_ok();
        match result {
            Ok(meshes) => {
                for (id, mesh) in meshes {
                    let original = before.0.objects.iter().find(|o| o.id == id);
                    if let (Some(o), Some(slot)) = (original, self.scene.get_mut(id)) {
                        *slot = o.clone();
                        slot.set_world_mesh(mesh);
                    }
                }
            }
            Err(e) => self.toast(e, true),
        }
    }

    /// Whether Apply can take the preview: it shows the settings now on the bar.
    pub(super) fn smooth_ready(&self) -> bool {
        matches!(&self.tool, Some(Tool::Smooth { amount, edges, run, .. })
            if run.ok && run.is_shown(*amount, *edges))
    }

    pub(super) fn apply_smooth(&mut self) {
        if !self.smooth_ready() {
            return;
        }
        if let Some(Tool::Smooth { before, .. }) = self.tool.take() {
            let (snapshot, depth, _) = *before;
            self.history.truncate(depth);
            self.history.push(snapshot);
            self.scene.dirty = true;
            self.refresh_invalid();
        }
    }
}
