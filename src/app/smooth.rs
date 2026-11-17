//! Edit → Smooth: the scene shows the smoothed meshes while the slider moves. The work runs
//! on a thread once the settings have rested for a moment.

use super::*;

/// What the Smooth tool has done so far: amount and whether edges are smoothed.
pub(super) type Run = live::Run<(f32, bool)>;

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
        let Some((gen, (amount, edges))) = run.next((*amount, *edges), ctx) else {
            return;
        };
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
            run.shown_now();
            return;
        }
        let work: Vec<_> = originals
            .iter()
            .map(|o| (o.id, o.mesh.clone(), o.matrix()))
            .collect();
        let triangles: usize = work.iter().map(|(_, m, _)| m.triangles.len()).sum();
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
        // A card for every slider stop would only flicker; big models take seconds, and
        // there the card says something is happening, as the original's does.
        job.hidden = triangles < 200_000;
        self.jobs.push(job);
    }

    /// A finished preview: shown unless the settings moved on.
    pub(super) fn smooth_done(&mut self, gen: u64, result: Result<Vec<(u64, Mesh)>, String>) {
        let Some(Tool::Smooth { before, run, .. }) = &mut self.tool else {
            return;
        };
        if !run.finish(gen, result.is_ok()) {
            return;
        }
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
            if run.ready((*amount, *edges)))
    }

    pub(super) fn apply_smooth(&mut self) {
        if !self.smooth_ready() {
            return;
        }
        if let Some(Tool::Smooth { before, .. }) = self.tool.take() {
            self.commit(*before);
            self.scene.dirty = true;
            self.refresh_invalid();
        }
    }
}
