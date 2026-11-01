//! 3D view: camera, picking, dragging and overlays.

use super::*;
use crate::snapping;
use std::sync::Arc;

#[path = "viewport_drag.rs"]
mod drag;
#[path = "viewport_frame.rs"]
mod frame;
#[path = "viewport_overlay.rs"]
mod overlay;

impl App {
    /// Shared traversal keeps object and face picking consistent.
    pub(super) fn pick_hit(&self, origin: Vec3, dir: Vec3) -> Option<(u64, usize, f32)> {
        type State = Vec<(u64, Arc<Mesh>, glam::Mat4)>;
        type Hit = Option<(u64, usize, f32)>;
        type Cached = (Vec3, Vec3, State, Hit);
        thread_local! { static HIT: std::cell::RefCell<Option<Cached>> = const { std::cell::RefCell::new(None) }; }
        let state: State = self
            .scene
            .objects
            .iter()
            .map(|o| (o.id, o.mesh.clone(), o.matrix()))
            .collect();
        if let Some(hit) = HIT.with(|cache| {
            cache
                .borrow()
                .as_ref()
                .filter(|(o, d, old, _)| {
                    *o == origin
                        && *d == dir
                        && old.len() == state.len()
                        && old
                            .iter()
                            .zip(&state)
                            .all(|(a, b)| a.0 == b.0 && Arc::ptr_eq(&a.1, &b.1) && a.2 == b.2)
                })
                .map(|c| c.3)
        }) {
            return hit;
        }
        let mut best: Option<(u64, usize, f32)> = None;
        for o in &self.scene.objects {
            if !ray_aabb(origin, dir, &o.world_aabb()) {
                continue;
            }
            let inv = o.matrix().inverse();
            let (lo, ld) = (inv.transform_point3(origin), inv.transform_vector3(dir));
            if let Some((i, t)) = self.picking.hit(&o.mesh, lo, ld) {
                let world_t = (o.matrix().transform_point3(lo + ld * t) - origin).length();
                if best.is_none_or(|(_, _, bt)| world_t < bt) {
                    best = Some((o.id, i, world_t));
                }
            }
        }
        HIT.with(|cache| *cache.borrow_mut() = Some((origin, dir, state, best)));
        best
    }

    fn pick_face(&self, origin: Vec3, dir: Vec3) -> Option<(u64, usize)> {
        self.pick_hit(origin, dir).map(|(id, tri, _)| (id, tri))
    }

    pub(super) fn pick(&self, origin: Vec3, dir: Vec3) -> Option<(u64, Vec3)> {
        self.pick_hit(origin, dir)
            .map(|(id, _, distance)| (id, origin + dir * distance))
    }

    /// The point a measure click would take: the surface under the ray, or the floor.
    fn measure_spot(&self, origin: Vec3, dir: Vec3) -> Option<Vec3> {
        let p = match self.pick(origin, dir) {
            Some((_, p)) => p,
            None => {
                let t = -origin.z / dir.z;
                (t.is_finite() && t > 0.0).then(|| origin + dir * t)?
            }
        };
        Some(if self.settings.snap {
            (p * 10.0).round() / 10.0
        } else {
            p
        })
    }

    pub(super) fn viewport(&mut self, ctx: &egui::Context) {
        if self.rotate_selection != self.scene.selection
            || (self.handle.is_none()
                && self
                    .scene
                    .selected()
                    .all(|o| o.rotation.abs().max_element() < 1e-4))
        {
            self.rotate_angles = [0.0; 3];
            self.rotate_selection = self.scene.selection.clone();
        }
        let lang = self.settings.lang();
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(VIEWPORT_BG))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                let resp = ui.allocate_rect(rect, Sense::click_and_drag());
                if let Some(Tool::Camera { cam, tex }) = &mut self.tool {
                    overlay::camera_feed(ui, rect, cam, tex, lang);
                    return;
                }
                let aspect = rect.width() / rect.height().max(1.0);
                self.aspect = aspect;
                self.view_rect = rect;
                self.camera_input(ui, &resp, rect);

                // The handles around the selection take the drag first; then dragging the
                // selection moves it on the floor; anything else orbits.
                let gizmo = self.gizmo(aspect, rect);
            // Capture the press event, before egui's drag threshold and any later
            // modifier changes. Only Alt starts a selection box.
            let marquee_modifier = ui.input(|i| i.modifiers.alt);
            let box_press = ui.input(|i| {
                i.events.iter().find_map(|event| match event {
                    egui::Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed: true,
                        modifiers,
                    } if rect.contains(*pos) => Some((*pos, modifiers.alt)),
                    _ => None,
                })
            });
                self.begin_drag(&resp, rect, aspect, gizmo.as_ref(), box_press, marquee_modifier);
                self.update_settle(ctx, ui, &resp, rect, aspect, box_press);
                let snap_guides = self.update_handle_drag(ui, &resp, rect, aspect);
            if resp.drag_started_by(PointerButton::Primary) {
                if let (Some(Tool::Emboss { .. }), Some(pos)) =
                    (&self.tool, resp.interact_pointer_pos())
                {
                    let (o, d) = self.camera.ray(to_ndc(rect, pos), aspect);
                    self.emboss_drag = self
                        .pick(o, d)
                        .is_some_and(|(id, _)| self.scene.selection.contains(&id));
                }
            }
            let texture = self.texture_pointer(ui, &resp, rect, aspect);
            let settling_grab = matches!(&self.tool, Some(Tool::Settle { bodies, .. }) if bodies.iter().any(|b| b.grab.is_some()));
            if self.handle.is_some() || settling_grab || texture.grab {
                // A transform handle or suspended body drives the pointer alone.
            } else if self.emboss_drag && resp.dragged_by(PointerButton::Primary) {
                    self.emboss_place(ui, &resp, rect, aspect);
                } else if self.marquee.is_some() {
                // The box is drawn with the overlays and applied when the drag stops.
            } else if resp.dragged_by(PointerButton::Primary) && !marquee_modifier {
                // A drag that starts off the selection and its handles orbits.
                let d = resp.drag_delta();
                self.glide = None;
                self.camera.orbit(GVec2::new(d.x, d.y));
            }
                self.finish_marquee(ui, &resp, rect, aspect);
                if resp.drag_stopped() {
                    self.finish_handle_drag();
                }
                if !ui.input(|i| i.pointer.primary_down()) {
                    self.marquee = None;
                }
                self.measure_hover(ui, &resp, rect, aspect);
                self.click(ui, &resp, rect, aspect, gizmo.as_ref());
                self.double_click(&resp, rect, aspect);
                let cb = self.frame(ui, rect, aspect, texture.preview);
                ui.painter()
                    .add(egui_wgpu::Callback::new_paint_callback(rect, cb));
                self.overlays(ui, &resp, rect, aspect, snap_guides);
            });
    }
}

fn to_ndc(rect: egui::Rect, p: Pos2) -> GVec2 {
    GVec2::new(
        (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
        1.0 - (p.y - rect.top()) / rect.height() * 2.0,
    )
}

/// Dragging one of the selection's handles.
pub(super) struct HandleDrag {
    /// The handle dragged; None when the selection itself is dragged on the floor.
    handle: Option<gizmo::Handle>,
    /// For a floor drag: the height of the plane the grabbed point slides on.
    plane_z: f32,
    /// Where the pointer was when the drag started, and last frame.
    from: Pos2,
    last: Pos2,
    /// Degrees turned so far, for turn handles that go round their pivot.
    turned: f32,
    /// The objects then, which every frame of the drag starts from.
    start: Vec<Object>,
    aabb: Aabb,
    /// The Split plane as the drag began.
    cut: Option<CutPlane>,
    before: Snapshot,
}
