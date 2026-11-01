//! Screenshot presets for the Texture tool (`--tool texture-menu` and so on).

use super::*;

/// A state to open the tool in, for screenshots (`--tool texture-menu` and so on).
#[derive(Clone, Copy)]
pub(crate) enum Preset {
    Menu,
    Sticker,
    Edit,
    Brush,
    /// Brush, then Split keeping the top half: textures must survive the cut.
    Split,
}

impl Preset {
    pub fn from_tool(name: &str) -> Self {
        match name {
            "texture-sticker" => Preset::Sticker,
            "texture-edit" => Preset::Edit,
            "texture-brush" => Preset::Brush,
            "texture-split" => Preset::Split,
            _ => Preset::Menu,
        }
    }
}

impl App {
    /// The triangle of the first selected (or first) object that faces the camera most, and
    /// the centre of the flat region around it.
    fn texture_facing(&self) -> Option<(u64, usize, Vec3)> {
        let o = self
            .scene
            .selected()
            .next()
            .or(self.scene.objects.first())?;
        let (model, eye) = (o.matrix(), self.camera.eye());
        let centre = |i: usize| {
            o.mesh.triangles[i]
                .map(|v| model.transform_point3(o.mesh.positions[v as usize]))
                .into_iter()
                .sum::<Vec3>()
                / 3.0
        };
        let score = |i: usize| {
            texture::world_normal(&o.mesh, i, &model).dot((eye - centre(i)).normalize_or_zero())
        };
        let tri = (0..o.mesh.triangles.len()).max_by(|a, b| score(*a).total_cmp(&score(*b)))?;
        let flat = o.mesh.face_region(tri, 5.0);
        let at = flat.iter().map(|&i| centre(i)).sum::<Vec3>() / flat.len().max(1) as f32;
        Some((o.id, tri, at))
    }

    /// Sets the tool up as a screenshot asks: menu open, a sticker placed or being edited,
    /// or faces painted.
    pub(in crate::app) fn texture_preset(&mut self, preset: Preset) {
        let Some(Tool::Texture(mut t)) = self.tool.take() else {
            return;
        };
        let hit = self.texture_facing();
        match (preset, hit) {
            (Preset::Menu, _) => t.open_menu(),
            (Preset::Sticker | Preset::Edit, Some(hit)) => {
                t.layer = texture::layer_of(Kind::Star8).unwrap_or(t.layer);
                let toward = hit.2 - self.camera.eye();
                if let Some(mut decal) = self.texture_decal(hit, t.layer, toward) {
                    decal.angle = 0.2;
                    let layer = t.layer;
                    self.texture_place(&mut t, hit.0, decal, layer);
                }
                if matches!(preset, Preset::Edit) {
                    self.texture_edit_sticker(&mut t);
                    if let Some(e) = &mut t.editing {
                        e.decal.width *= 1.3;
                        e.decal.height *= 1.3;
                        e.decal.angle = 0.5;
                    }
                }
            }
            (Preset::Brush | Preset::Split, Some(hit)) => {
                t.set_brush();
                t.coverage = 0.3;
                t.layer = texture::layer_of(Kind::Walnut).unwrap_or(t.layer);
                self.texture_dab(&t, hit, true);
                if let Some(o) = self.scene.get(hit.0) {
                    // The highest face too, so the brush shows on more than one side.
                    let model = o.matrix();
                    let top = (0..o.mesh.triangles.len()).max_by(|a, b| {
                        let key = |i: usize| {
                            let n = texture::world_normal(&o.mesh, i, &model);
                            (
                                n.z > 0.9,
                                o.mesh.triangles[i]
                                    .iter()
                                    .map(|&v| {
                                        model.transform_point3(o.mesh.positions[v as usize]).z
                                    })
                                    .sum::<f32>(),
                            )
                        };
                        let (x, y) = (key(*a), key(*b));
                        x.0.cmp(&y.0).then(x.1.total_cmp(&y.1))
                    });
                    if let Some(tri) = top {
                        let at = hit.2;
                        self.texture_dab(&t, (hit.0, tri, at), false);
                    }
                }
            }
            _ => {}
        }
        if matches!(preset, Preset::Split) {
            let z = self.scene.selection_aabb().center().z;
            let plane = crate::cut::CutPlane::at_height(z);
            self.edit(|s| s.cut_selected(&plane, crate::cut::Keep::Top));
            return;
        }
        self.tool = Some(Tool::Texture(t));
    }
}
