//! Edit → Smooth: the scene shows the smoothed meshes. The slider sets whole passes on
//! release, and the work runs on a thread once the settings have rested for a moment.

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
        if amount <= 0.0 {
            live::restore_all(&mut self.scene, before, &run.ids);
            run.shown_now();
            return;
        }
        let work: Vec<_> = live::originals(before, &run.ids)
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
        job.hidden = triangles < live::CARD_FROM;
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
                live::show_meshes(&mut self.scene, before, meshes, Object::set_world_mesh)
            }
            Err(e) => self.toast(e, true),
        }
    }
}
