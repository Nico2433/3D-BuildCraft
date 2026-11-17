//! Edit → Extrude outline: a border around (or inside) the selection's silhouette, previewed
//! in the scene while the settings change, as Smooth does.

use super::*;
use crate::outline::{Options, Outlined};

/// What the Extrude outline tool has done so far.
pub(super) type Run = live::Run<Options>;

/// The world meshes of the objects in `from` whose ids are in `ids`.
fn inputs(ids: &[u64], from: &[Object]) -> Vec<Mesh> {
    from.iter()
        .filter(|o| ids.contains(&o.id))
        .map(|o| o.world_mesh())
        .collect()
}

fn outline(meshes: &[Mesh], opts: Options) -> Result<Outlined, String> {
    crate::outline::outline(meshes, &opts).map_err(|e| format!("{e:#}"))
}

/// Puts the border in the scene for the objects `ids`. Replace: the first object takes the
/// new mesh and the others go. Beside: the objects stay and the border is added with the
/// first one's name and colour, selected with them.
fn place(scene: &mut Scene, ids: &[u64], outlined: Outlined) {
    let Some(first) = ids.first().and_then(|id| scene.get_mut(*id)) else {
        return;
    };
    match outlined {
        Outlined::Replace(mesh) => {
            first.set_world_mesh(mesh);
            scene.objects.retain(|o| !ids[1..].contains(&o.id));
            scene.selection.retain(|id| !ids[1..].contains(id));
        }
        Outlined::Beside(border) => {
            let border = Object::new(first.name.clone(), border, first.color);
            let new = scene.add(border);
            scene.selection.insert(new);
        }
    }
}

impl App {
    /// Opens the tool on the selection, its height that of the selection.
    pub(super) fn start_outline(&mut self) {
        if self.scene.selection.is_empty() {
            return;
        }
        // To a tenth of a millimetre, so it fits its field.
        let height = (self.scene.selection_aabb().size().z * 10.0)
            .round()
            .max(1.0)
            / 10.0;
        let opts = Options::new(height);
        // The scene shows no border yet (width 0); the first preview starts once they rest.
        let run = Run::new(self.scene.selected_ids(), Options { width: 0.0, ..opts });
        self.tool = Some(Tool::Outline {
            opts,
            before: self.checkpoint(),
            run,
        });
    }

    /// Starts the preview of the latest settings once they have rested.
    pub(super) fn outline_step(&mut self, ctx: &egui::Context) {
        let Some(Tool::Outline { opts, before, run }) = &mut self.tool else {
            return;
        };
        let Some((gen, opts)) = run.next(*opts, ctx) else {
            return;
        };
        let work = inputs(&run.ids, &before.0.objects);
        let mut job = jobs::Job::spawn(OUTLINING, 0, ctx, move |_| jobs::Done::Outlined {
            gen,
            result: outline(&work, opts),
        });
        // A card for every change would only flicker.
        job.hidden = true;
        self.jobs.push(job);
    }

    /// A finished preview: shown unless the settings moved on. The scene goes back to how
    /// it was before the tool, then takes the border.
    pub(super) fn outline_done(&mut self, gen: u64, result: Result<Outlined, String>) {
        let Some(Tool::Outline { before, run, .. }) = &mut self.tool else {
            return;
        };
        if !run.finish(gen, result.is_ok()) {
            return;
        }
        self.scene.objects = before.0.objects.clone();
        self.scene.selection = before.0.selection.clone();
        match result {
            Ok(outlined) => place(&mut self.scene, &run.ids, outlined),
            Err(e) => self.toast(e, true),
        }
    }

    /// The border at once, for the agent: one undo step.
    pub(super) fn outline_now(&mut self, opts: Options) -> Result<(), String> {
        let ids = self.scene.selected_ids();
        if ids.is_empty() {
            return Err("extrude_outline needs a selected object".into());
        }
        let outlined = outline(&inputs(&ids, &self.scene.objects), opts)?;
        self.edit(|s| place(s, &ids, outlined));
        self.refresh_invalid();
        Ok(())
    }
}
