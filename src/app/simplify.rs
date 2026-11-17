//! Edit → Simplify: the scene shows the reduced meshes, as the original's does. The slider
//! stops at whole percents and the work runs on a thread once it is released.

use super::*;

/// What the Simplify tool has done so far: the fraction of faces removed.
pub(super) type Run = live::Run<f32>;

impl App {
    /// Starts the preview of the latest level once it has rested.
    pub(super) fn simplify_step(&mut self, ctx: &egui::Context) {
        let Some(Tool::Simplify { level, before, run }) = &mut self.tool else {
            return;
        };
        let Some((gen, level)) = run.next(*level, ctx) else {
            return;
        };
        if level <= 0.0 {
            live::restore_all(&mut self.scene, before, &run.ids);
            run.shown_now();
            return;
        }
        let work: Vec<_> = live::originals(before, &run.ids)
            .map(|o| (o.id, o.mesh.clone()))
            .collect();
        let triangles: usize = work.iter().map(|(_, m)| m.triangles.len()).sum();
        let mut job = jobs::Job::spawn(SIMPLIFYING, 0, ctx, move |_| jobs::Done::Reduced {
            gen,
            result: work
                .into_iter()
                .map(|(id, mesh)| mesh.try_simplified(1.0 - level).map(|m| (id, m)))
                .collect::<anyhow::Result<_>>()
                .map_err(|e| format!("{e:#}")),
        });
        job.hidden = triangles < live::CARD_FROM;
        self.jobs.push(job);
    }

    /// A finished preview: shown unless the level moved on.
    pub(super) fn simplify_done(&mut self, gen: u64, result: Result<Vec<(u64, Mesh)>, String>) {
        let Some(Tool::Simplify { before, run, .. }) = &mut self.tool else {
            return;
        };
        if !run.finish(gen, result.is_ok()) {
            return;
        }
        match result {
            // Simplify keeps the object's placement and changes only its own mesh.
            Ok(meshes) => live::show_meshes(&mut self.scene, before, meshes, |o, mesh| {
                o.mesh = std::sync::Arc::new(mesh)
            }),
            Err(_) => {
                let lang = self.settings.lang();
                self.toast(tr(lang, "Simplification produced an invalid solid"), true)
            }
        }
    }

    /// The faces the scene shows for the tool's objects, and the ones they had before.
    pub(super) fn simplify_faces(&self) -> Option<(usize, usize)> {
        let Some(Tool::Simplify { before, run, .. }) = &self.tool else {
            return None;
        };
        let faces = |o: &Object| o.mesh.triangles.len();
        let shown = self
            .scene
            .objects
            .iter()
            .filter(|o| run.ids.contains(&o.id));
        Some((
            shown.map(faces).sum(),
            live::originals(before, &run.ids).map(faces).sum(),
        ))
    }
}
