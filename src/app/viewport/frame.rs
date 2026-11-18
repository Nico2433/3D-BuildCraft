//! The per-frame draw list and view settings handed to the GPU callback.

use super::*;
use crate::render::Slab;

impl App {
    /// What the GPU draws this frame: the scene plus any tool previews.
    pub(super) fn frame(
        &self,
        ui: &egui::Ui,
        rect: egui::Rect,
        aspect: f32,
        preview: Option<crate::render::DecalPreview>,
    ) -> FrameCallback {
        let r = self.settings.render;
        let custom = matches!(self.tool, Some(Tool::Custom { .. } | Tool::Image { .. }));
        // Behind a Custom preview or a file being imported, the scene is a see-through
        // blue, as in the original.
        let ghost = custom || !self.pending_import.is_empty();
        let mut items: Vec<DrawItem> = self
            .scene
            .objects
            .iter()
            .map(|o| DrawItem {
                mesh: o.mesh.clone(),
                model: o.matrix(),
                color: if ghost {
                    [0x8A, 0xA8, 0xF0, 110]
                } else if o.material == Material::Translucent {
                    [o.color[0], o.color[1], o.color[2], TRANSLUCENT_ALPHA]
                } else {
                    o.color
                },
                material: o.material,
                selected: self.scene.selection.contains(&o.id)
                    && !ghost
                    && !matches!(self.tool, Some(Tool::Settle { .. })),
            })
            .collect();
        if let Some(
            Tool::Custom {
                preview: Ok(shape), ..
            }
            | Tool::Image {
                preview: Ok(shape), ..
            },
        ) = &self.tool
        {
            // First, so the see-through scene blends over it instead of hiding it.
            items.insert(
                0,
                DrawItem {
                    mesh: shape.clone(),
                    model: glam::Mat4::IDENTITY,
                    color: [0xC8, 0xC8, 0xC8, 0xFF],
                    selected: false,
                    material: Material::None,
                },
            );
        }
        // Files waiting for their units show where they will land, at the chosen size.
        // First, so the see-through scene blends over them instead of hiding them.
        let waiting: Vec<DrawItem> = self
            .shown_imports()
            .flat_map(|p| p.placed())
            .map(|(mesh, model, color)| DrawItem {
                mesh,
                model,
                color,
                selected: false,
                material: Material::None,
            })
            .collect();
        items.splice(0..0, waiting);
        let cb = FrameCallback {
            repaint: ui.ctx().clone(),
            items,
            decal: preview,
            view_proj: self.camera.view_proj(aspect),
            eye: self.camera.eye(),
            viewport: glam::Vec2::new(rect.width(), rect.height()) * ui.ctx().pixels_per_point(),
            slabs: self
                .cut_plane_slab()
                .into_iter()
                .chain(self.repair_bands())
                .collect(),
            lines: self.emboss_lines(ui.ctx().pixels_per_point()),
            // The Split preview shows only the part that will be kept.
            clip: match &self.tool {
                Some(Tool::Cut { plane, keep }) if *keep != Keep::Both => {
                    let (u, v) = plane.axes();
                    Some(crate::render::Clip {
                        point: plane.point(),
                        normal: plane.normal() * if *keep == Keep::Top { -1.0 } else { 1.0 },
                        u,
                        v,
                        half: plane.half_size(),
                    })
                }
                _ => None,
            },
            flags: ViewFlags {
                shading: r.shading,
                smoothing: r.smoothing,
                // The original shows the object in x-ray while hollowing.
                xray: r.xray || matches!(self.tool, Some(Tool::Hollow { .. })),
                colors: r.colors,
                // The original shows the mesh while smoothing and simplifying.
                wireframe: r.wireframe
                    || matches!(
                        self.tool,
                        Some(
                            Tool::Simplify { .. }
                                | Tool::Smooth { .. }
                                | Tool::Custom { wire: true, .. }
                        )
                    ),
                floor: r.grid,
                floor_tone: r.grid_tone,
                shadows: r.shadows,
                reflections: r.reflections,
            },
        };
        cb
    }

    /// The Split plane, green to move and blue to turn.
    fn cut_plane_slab(&self) -> Option<Slab> {
        let Some(Tool::Cut { plane, .. }) = &self.tool else {
            return None;
        };
        let c = self.cut_slab(plane);
        let model = glam::Mat4::from_cols(
            ((c[1] - c[0]) * 0.5).extend(0.0),
            ((c[3] - c[0]) * 0.5).extend(0.0),
            Vec3::Z.extend(0.0),
            ((c[0] + c[2]) * 0.5).extend(1.0),
        );
        let (r, g, b) = if self.mode == TransformMode::Rotate {
            (0x1E, 0xA0, 0xE8)
        } else {
            (0x4C, 0xC0, 0x4C)
        };
        let to_f = |x: u8| x as f32 / 255.0;
        Some(Slab {
            model,
            color: [to_f(r), to_f(g), to_f(b), 0.35],
            edge: true,
        })
    }

    /// The original marks an object that needs repair with a translucent red band on the
    /// floor around its base, a little outside it. It lies in the scene, so the object
    /// hides the part behind it and the far side looks thinner.
    fn repair_bands(&self) -> Vec<Slab> {
        // Matched to the original's pixels: the band is about 3 % of the model's longest
        // side wide and stands off its box by as much; red at 60 % over the floor.
        const COLOR: [f32; 4] = [1.0, 0.18, 0.18, 0.6];
        let rect = |min: glam::Vec2, max: glam::Vec2, z: f32| Slab {
            model: glam::Mat4::from_cols(
                glam::Vec4::new((max.x - min.x) * 0.5, 0.0, 0.0, 0.0),
                glam::Vec4::new(0.0, (max.y - min.y) * 0.5, 0.0, 0.0),
                glam::Vec4::Z,
                ((min + max) * 0.5).extend(z).extend(1.0),
            ),
            color: COLOR,
            edge: false,
        };
        let mut out = Vec::new();
        for o in self.invalid.iter().filter_map(|id| self.scene.get(*id)) {
            let b = o.world_aabb();
            if !b.is_valid() {
                continue;
            }
            let (lo, hi) = (b.min.truncate(), b.max.truncate());
            let w = ((hi - lo).max_element() * 0.03).clamp(1.0, 6.0);
            let gap = glam::Vec2::splat(w);
            let (inner_lo, inner_hi) = (lo - gap, hi + gap);
            let (outer_lo, outer_hi) = (
                inner_lo - glam::Vec2::splat(w),
                inner_hi + glam::Vec2::splat(w),
            );
            // Just above the floor (drawn at -0.02) so it is not lost in it.
            let z = b.min.z + 0.02;
            // Front and back span the full width; the sides fit between them, so the
            // corners are not blended twice.
            out.extend([
                rect(outer_lo, glam::Vec2::new(outer_hi.x, inner_lo.y), z),
                rect(glam::Vec2::new(outer_lo.x, inner_hi.y), outer_hi, z),
                rect(
                    glam::Vec2::new(outer_lo.x, inner_lo.y),
                    glam::Vec2::new(inner_lo.x, inner_hi.y),
                    z,
                ),
                rect(
                    glam::Vec2::new(inner_hi.x, inner_lo.y),
                    glam::Vec2::new(outer_hi.x, inner_hi.y),
                    z,
                ),
            ]);
        }
        out
    }
}
