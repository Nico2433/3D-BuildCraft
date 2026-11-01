//! The per-frame draw list and view settings handed to the GPU callback.

use super::*;

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
        // A file waiting for its units shows where it will land, at the chosen size.
        // First, so the see-through scene blends over it instead of hiding it.
        if let Some(p) = self.pending_import.first() {
            items.splice(
                0..0,
                p.placed().map(|(mesh, model, color)| DrawItem {
                    mesh,
                    model,
                    color,
                    selected: false,
                    material: Material::None,
                }),
            );
        }
        if let Some(Tool::Emboss {
            preview: Ok(text),
            params,
            ..
        }) = &self.tool
        {
            items.push(DrawItem {
                mesh: text.clone(),
                model: glam::Mat4::IDENTITY,
                color: params
                    .top_color
                    .filter(|_| !params.engrave)
                    .unwrap_or([0xF7, 0x8B, 0x1F, 0xFF]),
                selected: false,
                material: Material::None,
            });
        }
        let cb = FrameCallback {
            items,
            decal: preview,
            view_proj: self.camera.view_proj(aspect),
            eye: self.camera.eye(),
            viewport: glam::Vec2::new(rect.width(), rect.height()) * ui.ctx().pixels_per_point(),
            // The cutting plane, green to move and blue to turn.
            slab: match &self.tool {
                Some(Tool::Cut { plane, .. }) => {
                    let c = self.cut_slab(plane);
                    let m = glam::Mat4::from_cols(
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
                    Some((m, [to_f(r), to_f(g), to_f(b), 0.35]))
                }
                _ => None,
            },
            // The Split preview shows only the part that will be kept.
            clip: match &self.tool {
                Some(Tool::Cut { plane, keep }) if *keep != Keep::Both => {
                    let n = plane.normal() * if *keep == Keep::Top { -1.0 } else { 1.0 };
                    Some((n, n.dot(plane.point())))
                }
                _ => None,
            },
            flags: ViewFlags {
                shading: r.shading,
                smoothing: r.smoothing,
                // The original shows the object in x-ray while hollowing.
                xray: r.xray || matches!(self.tool, Some(Tool::Hollow { .. })),
                colors: r.colors,
                wireframe: r.wireframe
                    || matches!(
                        self.tool,
                        Some(Tool::Simplify { .. } | Tool::Custom { wire: true, .. })
                    ),
                floor: r.grid,
                floor_tone: r.grid_tone,
                shadows: r.shadows,
                reflections: r.reflections,
            },
        };
        cb
    }
}
