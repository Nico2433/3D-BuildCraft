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
        let originals: Vec<&Object> = before
            .0
            .objects
            .iter()
            .filter(|o| run.ids.contains(&o.id))
            .collect();
        if level <= 0.0 {
            for o in originals {
                if let Some(slot) = self.scene.get_mut(o.id) {
                    *slot = o.clone();
                }
            }
            run.shown_now();
            return;
        }
        let work: Vec<_> = originals.iter().map(|o| (o.id, o.mesh.clone())).collect();
        let triangles: usize = work.iter().map(|(_, m)| m.triangles.len()).sum();
        let mut job = jobs::Job::spawn(SIMPLIFYING, 0, ctx, move |_| jobs::Done::Reduced {
            gen,
            result: work
                .into_iter()
                .map(|(id, mesh)| mesh.try_simplified(1.0 - level).map(|m| (id, m)))
                .collect::<anyhow::Result<_>>()
                .map_err(|e| format!("{e:#}")),
        });
        // Small models answer at once; a card for them would only flicker.
        job.hidden = triangles < 200_000;
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
            Ok(meshes) => {
                for (id, mesh) in meshes {
                    let original = before.0.objects.iter().find(|o| o.id == id);
                    if let (Some(o), Some(slot)) = (original, self.scene.get_mut(id)) {
                        *slot = o.clone();
                        slot.mesh = std::sync::Arc::new(mesh);
                    }
                }
            }
            Err(_) => {
                let lang = self.settings.lang();
                self.toast(tr(lang, "Simplification produced an invalid solid"), true)
            }
        }
    }

    /// Whether Apply can take the preview: it shows the level now on the bar.
    pub(super) fn simplify_ready(&self) -> bool {
        matches!(&self.tool, Some(Tool::Simplify { level, run, .. }) if run.ready(*level))
    }

    /// The faces the scene shows for the tool's objects, and the ones they had before.
    pub(super) fn simplify_faces(&self) -> Option<(usize, usize)> {
        let Some(Tool::Simplify { before, run, .. }) = &self.tool else {
            return None;
        };
        let count = |objects: &mut dyn Iterator<Item = &Object>| {
            objects
                .filter(|o| run.ids.contains(&o.id))
                .map(|o| o.mesh.triangles.len())
                .sum()
        };
        Some((
            count(&mut self.scene.objects.iter()),
            count(&mut before.0.objects.iter()),
        ))
    }

    pub(super) fn apply_simplify(&mut self) {
        if !self.simplify_ready() {
            return;
        }
        if let Some(Tool::Simplify { before, .. }) = self.tool.take() {
            self.commit(*before);
            self.scene.dirty = true;
            self.refresh_invalid();
        }
    }
}
