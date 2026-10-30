//! 3D view: camera, picking, dragging and overlays.

use super::*;
use crate::snapping;
use std::sync::Arc;

impl App {
    /// Shared traversal keeps object and face picking consistent.
    fn pick_hit(&self, origin: Vec3, dir: Vec3) -> Option<(u64, usize, f32)> {
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
                    camera_feed(ui, rect, cam, tex, lang);
                    return;
                }
                let aspect = rect.width() / rect.height().max(1.0);
                self.aspect = aspect;
                self.view_rect = rect;
                let to_ndc = |p: Pos2| {
                    GVec2::new(
                        (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
                        1.0 - (p.y - rect.top()) / rect.height() * 2.0,
                    )
                };

                // Camera: left drag on empty space orbits, right/middle drag pans, wheel zooms.
                let scroll = ui.input(|i| i.smooth_scroll_delta.y);
                if resp.hovered() && scroll != 0.0 {
                    self.glide = None;
                    self.camera.zoom(scroll);
                }
                if resp.dragged_by(PointerButton::Secondary)
                    || resp.dragged_by(PointerButton::Middle)
                {
                    let d = resp.drag_delta();
                    self.glide = None;
                    self.camera.pan(GVec2::new(d.x, d.y), rect.height());
                }

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
                if self.tool.is_none() && resp.is_pointer_button_down_on() {
                    if let Some((pos, true)) = box_press {
                        self.marquee = Some(pos);
                    }
                }
                if resp.drag_started_by(PointerButton::Primary)
                    && self.tool.is_none()
                    && self.marquee.is_none()
                    && !marquee_modifier
                {
                    let at = resp.interact_pointer_pos();
                    let grabbed = at.zip(gizmo.as_ref()).and_then(|(p, g)| g.grab(p));
                    // Pressed on a selected object (and no handle): drag it on the floor.
                    let on_selection = at
                        .filter(|_| grabbed.is_none())
                        .and_then(|p| {
                            let (o, d) = self.camera.ray(to_ndc(p), aspect);
                            self.pick(o, d)
                        })
                        .filter(|(id, _)| self.scene.selection.contains(id));
                    if let (Some((_, hit)), Some(from)) = (on_selection, at) {
                        self.handle = Some(HandleDrag {
                            handle: None,
                            plane_z: hit.z,
                            from,
                            last: from,
                            turned: 0.0,
                            start: self.scene.objects.clone(),
                            aabb: self.scene.selection_aabb(),
                            before: self.snapshot(),
                        });
                    }
                    if let (Some(handle), Some(from)) = (grabbed, at) {
                        self.pick_handle(handle.kind);
                        let before = self.snapshot();
                        if matches!(handle.kind, gizmo::Kind::Size(..) | gizmo::Kind::Uniform) {
                            // Resizing works on the box as it is, so turned objects are
                            // baked first (resize_selection would bake them on every frame).
                            for id in self.scene.selected_ids() {
                                let o = self.scene.get_mut(id).unwrap();
                                if o.rotation != Vec3::ZERO {
                                    o.bake();
                                }
                            }
                        }
                        self.handle = Some(HandleDrag {
                            handle: Some(handle),
                            plane_z: 0.0,
                            from,
                            last: from,
                            turned: 0.0,
                            start: self.scene.objects.clone(),
                            aabb: self.scene.selection_aabb(),
                            before,
                        });
                    }
                }
                let mut snap_guides = Vec::new();
                self.update_settle(ctx, ui, &resp, rect, aspect, box_press);
                // Every frame of a drag starts again from the objects as they were; if
                // something else (the assistant) changed the scene meanwhile, the drag
                // ends rather than undo that change.
                let changed_meanwhile = self.handle.as_ref().is_some_and(|h| {
                    h.start.len() != self.scene.objects.len()
                        || h.start
                            .iter()
                            .zip(&self.scene.objects)
                            .any(|(a, b)| a.id != b.id || !std::sync::Arc::ptr_eq(&a.mesh, &b.mesh))
                });
                if changed_meanwhile {
                    self.handle = None;
                }
                if let (Some(h), Some(p)) = (&mut self.handle, resp.interact_pointer_pos()) {
                    if let Some(handle) = h.handle {
                        // Turns add up frame by frame so a drag can go past half a turn.
                        let amount = match handle.turned(h.last, p) {
                            Some(a) => {
                                h.turned += a;
                                h.turned
                            }
                            None => {
                                h.turned = handle.amount(p - h.from);
                                h.turned
                            }
                        };
                        h.last = p;
                        let (start, b0) = (h.start.clone(), h.aabb);
                        self.drag_handle(handle.kind, amount, start, &b0, ui.input(|i| i.modifiers.alt));
                    } else {
                        // The grabbed point follows the pointer on its horizontal plane.
                        let on_plane = |q: Pos2| {
                            let (o, d) = self.camera.ray(to_ndc(q), aspect);
                            let t = (h.plane_z - o.z) / d.z;
                            (d.z.abs() > 1e-6 && t > 0.0).then(|| o + d * t)
                        };
                        if let (Some(a), Some(b)) = (on_plane(h.from), on_plane(p)) {
                            let mut shift = b - a;
                            shift.z = 0.0;
                            if self.settings.snap && !ui.input(|i| i.modifiers.alt) {
                                shift = shift.round();
                            }
                            self.scene.objects = h.start.clone();
                            for id in self.scene.selected_ids() {
                                self.scene.get_mut(id).unwrap().translation += shift;
                            }
                            self.scene.dirty = true;
                        }
                    }
                }
                if let Some(h) = &self.handle {
                    let axes: &[usize] = match h.handle.map(|h| h.kind) {
                        None => &[0, 1],
                        Some(gizmo::Kind::Move(0)) => &[0],
                        Some(gizmo::Kind::Move(1)) => &[1],
                        Some(gizmo::Kind::Move(2)) => &[2],
                        _ => &[],
                    };
                    if !ui.input(|i| i.modifiers.alt) {
                        snap_guides = self.snap_movement(axes, rect, aspect);
                    }
                }
                if resp.drag_started_by(PointerButton::Primary) {
                    if let (Some(Tool::Emboss { .. }), Some(pos)) =
                        (&self.tool, resp.interact_pointer_pos())
                    {
                        let (o, d) = self.camera.ray(to_ndc(pos), aspect);
                        self.emboss_drag = self
                            .pick(o, d)
                            .is_some_and(|(id, _)| self.scene.selection.contains(&id));
                    }
                }
                let settling_grab = matches!(&self.tool, Some(Tool::Settle { bodies, .. }) if bodies.iter().any(|b| b.grab.is_some()));
                if self.handle.is_some() || settling_grab {
                    // A transform handle or suspended body drives the pointer alone.
                } else if self.emboss_drag && resp.dragged_by(PointerButton::Primary) {
                    let shift = ui.input(|i| i.modifiers.shift);
                    let (o, d) = match resp.interact_pointer_pos() {
                        Some(pos) => self.camera.ray(to_ndc(pos), aspect),
                        None => (Vec3::ZERO, Vec3::Z),
                    };
                    let hit = self.pick(o, d);
                    let target = self
                        .scene
                        .selected()
                        .next()
                        .map(|o| (o.id, o.mesh.clone(), o.matrix()));
                    if let Some(Tool::Emboss { params, .. }) = &mut self.tool {
                        if shift {
                            params.angle = (params.angle + resp.drag_delta().x * 0.5 + 180.0)
                                .rem_euclid(360.0)
                                - 180.0;
                        } else if let (Some((hit_id, p)), Some((id, mesh, matrix))) = (hit, target)
                        {
                            if hit_id == id {
                                params.offset = crate::emboss::offset_at_placed(
                                    &mesh,
                                    matrix,
                                    params.projection,
                                    p,
                                );
                            }
                        }
                    }
                } else if self.marquee.is_some() {
                    // The box is drawn with the overlays and applied when the drag stops.
                } else if resp.dragged_by(PointerButton::Primary) && !marquee_modifier {
                    // A drag that starts off the selection and its handles orbits.
                    let d = resp.drag_delta();
                    self.glide = None;
                    self.camera.orbit(GVec2::new(d.x, d.y));
                }
                if let Some(from) = ui
                    .input(|i| i.pointer.primary_released())
                    .then(|| self.marquee.take())
                    .flatten()
                {
                    // Ignore a press without a selection-box drag.
                    if let Some(to) = resp
                        .interact_pointer_pos()
                        .or(resp.hover_pos())
                        .filter(|to| from.distance(*to) > 4.0)
                    {
                        let area = egui::Rect::from_two_pos(from, to);
                        let add = ui.input(|i| i.modifiers.shift);
                        if !add {
                            self.scene.selection.clear();
                        }
                        let hit: Vec<u64> = self
                            .scene
                            .objects
                            .iter()
                            .filter(|o| {
                                self.screen_box(&o.world_aabb(), rect, aspect)
                                    .is_some_and(|b| b.intersects(area))
                            })
                            .map(|o| o.id)
                            .collect();
                        self.scene.selection.extend(hit);
                    }
                }
                if resp.drag_stopped() {
                    // One undo step for the whole handle drag, none when nothing changed.
                    if let Some(h) = self.handle.take() {
                        if let Some(gizmo::Handle {
                            kind: gizmo::Kind::Turn(axis),
                            ..
                        }) = h.handle
                        {
                            self.rotate_angles[axis] = (self.rotate_angles[axis]
                                + self.rotation_drag_angle(h.turned))
                            .rem_euclid(360.0);
                        }
                        let same = |a: &Object, b: &Object| {
                            a.id == b.id
                                && a.translation == b.translation
                                && a.rotation == b.rotation
                                && a.scale == b.scale
                                && std::sync::Arc::ptr_eq(&a.mesh, &b.mesh)
                        };
                        let objects = &self.scene.objects;
                        let changed = objects.len() != h.before.objects.len()
                            || objects
                                .iter()
                                .zip(&h.before.objects)
                                .any(|(a, b)| !same(a, b));
                        if changed {
                            self.history.push(h.before);
                        }
                    }
                    self.emboss_drag = false;
                }

                if !ui.input(|i| i.pointer.primary_down()) {
                    self.marquee = None;
                }

                // A click on a handle picks it (and its field) instead of selecting.
                let clicked_handle = resp
                    .clicked()
                    .then(|| resp.interact_pointer_pos().zip(gizmo.as_ref()))
                    .flatten()
                    .and_then(|(p, g)| g.grab(p).filter(|_| self.tool.is_none()));
                if let Some(h) = clicked_handle {
                    self.pick_handle(h.kind);
                } else if resp.clicked() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (o, d) = self.camera.ray(to_ndc(pos), aspect);
                        let hit = self.pick(o, d);
                        let face = matches!(self.tool, Some(Tool::Paint { .. }))
                            .then(|| self.pick_face(o, d))
                            .flatten();
                        match &mut self.tool {
                            Some(Tool::Measure { points }) => {
                                if let Some((_, mut p)) = hit {
                                    if self.settings.snap {
                                        p = (p * 10.0).round() / 10.0;
                                    }
                                    if points.len() >= 2 {
                                        points.clear();
                                    }
                                    points.push(p);
                                }
                            }
                            Some(Tool::Paint {
                                color,
                                coverage,
                                dropper,
                                ..
                            }) => {
                                if let Some((id, tri)) = face {
                                    if *dropper {
                                        // The eyedropper takes the clicked face's colour.
                                        if let Some(o) = self.scene.get(id) {
                                            *color = o.mesh.face_color(tri).unwrap_or(o.color);
                                        }
                                        *dropper = false;
                                    } else {
                                        self.actions
                                            .push(Action::PaintArea(id, tri, *color, *coverage));
                                    }
                                }
                            }
                            Some(
                                Tool::Settle { .. }
                                | Tool::Cut { .. }
                                | Tool::Simplify { .. }
                                | Tool::Hollow { .. }
                                | Tool::Emboss { .. }
                                | Tool::Custom { .. }
                                | Tool::Image { .. }
                                | Tool::Camera { .. },
                            ) => {}
                            None => {
                                let add = ui.input(|i| {
                                    i.modifiers.command || i.modifiers.ctrl || i.modifiers.shift
                                });
                                match hit {
                                    Some((id, _)) => self.actions.push(Action::Select(id, add)),
                                    None if !add => self.scene.selection.clear(),
                                    None => {}
                                }
                            }
                        }
                    }
                }

                if resp.double_clicked() && self.tool.is_none() {
                    if let Some(pos) = resp.interact_pointer_pos() {
                        let (o, d) = self.camera.ray(to_ndc(pos), aspect);
                        // A double click centres the view on the object, as in the original.
                        if let Some(b) = self
                            .pick(o, d)
                            .and_then(|(id, _)| self.scene.get(id))
                            .map(|o| o.world_aabb())
                        {
                            self.glide_to(&b);
                        }
                    }
                }

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
                        } else {
                            o.color
                        },
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
                    });
                }
                let cb = FrameCallback {
                    items,
                    view_proj: self.camera.view_proj(aspect),
                    eye: self.camera.eye(),
                    viewport: glam::Vec2::new(rect.width(), rect.height())
                        * ui.ctx().pixels_per_point(),
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
                ui.painter()
                    .add(egui_wgpu::Callback::new_paint_callback(rect, cb));

                // 2D overlays.
                let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
                let painter = ui.painter_at(rect);
                for (a, b) in snap_guides {
                    if let (Some(a), Some(b)) = (to_screen(a), to_screen(b)) {
                        painter.line_segment([a, b], Stroke::new(1.0_f32, Color32::from_rgb(90, 210, 245)));
                    }
                }
                if r.grid && r.grid_labels {
                    // Distances along the X and Y axes, written on the floor and smaller
                    // far away.
                    for (axis, k) in [Vec3::X, Vec3::Y]
                        .into_iter()
                        .flat_map(|a| (1..=12).map(move |k| (a, k)))
                    {
                        let p = axis * (k as f32 * 50.0);
                        let Some(s) = to_screen(p) else { continue };
                        // Hidden behind objects, roughly: their boxes block the label.
                        let eye = self.camera.eye();
                        let dir = (p - eye).normalize();
                        let far = eye.distance(p);
                        let hidden = self.scene.objects.iter().any(|o| {
                            let b = o.world_aabb();
                            ray_aabb(eye, dir, &b) && b.center().distance(eye) < far
                        });
                        if hidden {
                            continue;
                        }
                        let size = 22.0 * 260.0 / self.camera.eye().distance(p);
                        // Too far to read: they would only pile up at the horizon.
                        if size < 9.0 {
                            continue;
                        }
                        let size = size.min(22.0);
                        painter.text(
                            s,
                            Align2::CENTER_CENTER,
                            format!("{} mm", k * 50),
                            egui::FontId::proportional(size),
                            Color32::from_rgba_unmultiplied(0x2A, 0x2A, 0x2A, 190),
                        );
                    }
                }
                if let (Some(from), Some(to)) = (self.marquee, resp.interact_pointer_pos()) {
                    let area = egui::Rect::from_two_pos(from, to);
                    painter.rect_filled(
                        area,
                        0.0,
                        Color32::from_rgba_unmultiplied(0x1E, 0xA0, 0xE8, 40),
                    );
                    painter.rect_stroke(
                        area,
                        0.0,
                        Stroke::new(1.0_f32, Color32::from_rgb(0x1E, 0xA0, 0xE8)),
                        egui::StrokeKind::Inside,
                    );
                }
                if let Some(g) = self.gizmo(aspect, rect) {
                    // The handle being dragged, or the one under the pointer, is green and
                    // the pointer becomes a hand over it.
                    let hot = self
                        .handle
                        .as_ref()
                        .and_then(|h| h.handle.map(|h| h.kind))
                        .or_else(|| resp.hover_pos().and_then(|p| g.grab(p)).map(|h| h.kind));
                    // An open hand over a handle or the selection, a closed one dragging.
                    let over_selection = || {
                        resp.hover_pos().is_some_and(|p| {
                            let (o, d) = self.camera.ray(to_ndc(p), aspect);
                            self.pick(o, d)
                                .is_some_and(|(id, _)| self.scene.selection.contains(&id))
                        })
                    };
                    let pressed = ui.input(|i| i.pointer.primary_down());
                    if self.handle.is_some() {
                        icons::hand_cursor(ui.ctx(), true);
                    } else if hot.is_some() || over_selection() {
                        // Held down on them, the hand closes before the drag starts.
                        icons::hand_cursor(ui.ctx(), pressed);
                    }
                    g.paint(&painter, hot, self.field);
                }
                for id in &self.invalid {
                    if let Some(o) = self.scene.get(*id) {
                        draw_box(
                            &painter,
                            &o.world_aabb(),
                            Stroke::new(2.0_f32, Color32::from_rgb(0xE0, 0x30, 0x30)),
                            &to_screen,
                        );
                    }
                }
                if let Some(Tool::Cut { z, .. }) = &self.tool {
                    // The cutting plane, a little wider than the selection.
                    let b = self.scene.selection_aabb();
                    let pad = b.size().max_element() * 0.15;
                    let (lo, hi) = (b.min - pad, b.max + pad);
                    let corners = [(lo.x, lo.y), (hi.x, lo.y), (hi.x, hi.y), (lo.x, hi.y)];
                    let pts: Vec<Pos2> = corners
                        .iter()
                        .filter_map(|(x, y)| to_screen(Vec3::new(*x, *y, *z)))
                        .collect();
                    if pts.len() == 4 {
                        painter.add(egui::Shape::convex_polygon(
                            pts,
                            Color32::from_rgba_unmultiplied(0x4C, 0xC0, 0x4C, 90),
                            Stroke::new(1.5_f32, Color32::from_rgb(0x4C, 0xE0, 0x4C)),
                        ));
                    }
                }
                if let Some(Tool::Measure { points }) = &self.tool {
                    let pts: Vec<Pos2> = points.iter().filter_map(|p| to_screen(*p)).collect();
                    for p in &pts {
                        painter.circle_filled(*p, 5.0, Color32::from_rgb(0x4C, 0xE0, 0x4C));
                    }
                    if let [a, b] = pts[..] {
                        painter.line_segment(
                            [a, b],
                            Stroke::new(2.0_f32, Color32::from_rgb(0x4C, 0xE0, 0x4C)),
                        );
                    }
                }
            });
    }
}

impl App {
    fn rotation_drag_angle(&self, degrees: f32) -> f32 {
        if self.settings.snap {
            (degrees / 5.0).round() * 5.0
        } else {
            degrees
        }
    }

    /// The handles around the selection, in screen points; `None` when nothing is
    /// selected or a tool or the print view is open.
    fn gizmo(&self, aspect: f32, rect: egui::Rect) -> Option<gizmo::Gizmo> {
        if self.scene.selection.is_empty() || self.tool.is_some() || self.print.is_some() {
            return None;
        }
        let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
        let active = self
            .handle
            .as_ref()
            .filter(|h| matches!(h.handle.map(|h| h.kind), Some(gizmo::Kind::Turn(_))));
        let b = self.scene.selection_aabb();
        let mut angles = self.rotate_angles;
        if let Some(h) = active {
            if let Some(gizmo::Handle {
                kind: gizmo::Kind::Turn(axis),
                ..
            }) = h.handle
            {
                angles[axis] += self.rotation_drag_angle(h.turned);
            }
        }
        Some(gizmo::build(
            self.mode,
            &b,
            self.camera.eye(),
            angles,
            &to_screen,
        ))
    }

    /// A point of the scene on screen in the view `rect`, if it is in front of the camera.
    fn to_screen(&self, p: Vec3, rect: egui::Rect, aspect: f32) -> Option<Pos2> {
        self.camera.project(p, aspect).map(|n| {
            Pos2::new(
                rect.left() + (n.x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - n.y) * 0.5 * rect.height(),
            )
        })
    }

    /// Where a box appears on screen in the view `rect`: the bounds of its corners in front
    /// of the camera.
    fn screen_box(&self, b: &Aabb, rect: egui::Rect, aspect: f32) -> Option<egui::Rect> {
        let pts: Vec<Pos2> = (0..8)
            .filter_map(|i| {
                let c = Vec3::new(
                    if i & 1 != 0 { b.max.x } else { b.min.x },
                    if i & 2 != 0 { b.max.y } else { b.min.y },
                    if i & 4 != 0 { b.max.z } else { b.min.z },
                );
                self.to_screen(c, rect, aspect)
            })
            .collect();
        (!pts.is_empty()).then(|| egui::Rect::from_points(&pts))
    }

    /// Picks a handle: the bottom bar's field for its axis becomes the current one, and
    /// with it the handle stays green.
    fn pick_handle(&mut self, kind: gizmo::Kind) {
        if let Some(f) = kind.field() {
            self.field = f;
        }
    }

    /// Convert eight logical screen pixels at the selection depth into millimetres.
    fn snap_movement(
        &mut self,
        axes: &[usize],
        rect: egui::Rect,
        aspect: f32,
    ) -> Vec<(Vec3, Vec3)> {
        let targets: Vec<_> = self
            .scene
            .objects
            .iter()
            .filter(|o| !self.scene.selection.contains(&o.id))
            .map(Object::world_aabb)
            .collect();
        let mut guides = Vec::new();
        for &axis in axes {
            let bounds = self.scene.selection_aabb();
            let centre = bounds.center();
            let mut direction = Vec3::ZERO;
            direction[axis] = 1.0;
            let pixels_per_mm = self
                .to_screen(centre, rect, aspect)
                .zip(self.to_screen(centre + direction, rect, aspect))
                .map(|(a, b)| a.distance(b))
                .unwrap_or(0.0);
            // An axis viewed end-on has no useful screen-space snapping distance.
            if pixels_per_mm < 0.01 {
                continue;
            }
            if let Some(hit) = snapping::snap_axis(&bounds, &targets, axis, 8.0 / pixels_per_mm) {
                if axis == 2 && bounds.min.z >= -1e-4 && bounds.min.z + hit.offset < -1e-4 {
                    continue;
                }
                for id in self.scene.selected_ids() {
                    self.scene.get_mut(id).unwrap().translation[axis] += hit.offset;
                }
                self.scene.dirty = true;
                let mut a = centre;
                let mut b = hit.target.center();
                a[axis] = hit.value;
                b[axis] = hit.value;
                let along = (b - a).normalize_or_zero();
                let pad = bounds
                    .size()
                    .max_element()
                    .max(hit.target.size().max_element())
                    * 0.6;
                guides.push((a - along * pad, b + along * pad));
            }
        }
        guides
    }

    /// Applies a handle drag of `amount` (mm, degrees or a size factor) to the objects as
    /// they were when it started.
    fn drag_handle(
        &mut self,
        kind: gizmo::Kind,
        amount: f32,
        start: Vec<Object>,
        b0: &Aabb,
        unsnapped: bool,
    ) {
        use gizmo::Kind;
        let snap = self.settings.snap && !unsnapped;
        self.scene.objects = start;
        let ids = self.scene.selected_ids();
        match kind {
            Kind::Move(axis) => {
                let mut d = if snap { amount.round() } else { amount };
                // Not below the floor, unless it already was.
                if axis == 2 && b0.min.z >= -1e-4 {
                    d = d.max(-b0.min.z);
                }
                for id in &ids {
                    self.scene.get_mut(*id).unwrap().translation[axis] += d;
                }
            }
            Kind::Turn(axis) => {
                let deg = self.rotation_drag_angle(amount);
                // About the world axis through the selection's centre, so it turns in place.
                let mut along = Vec3::ZERO;
                along[axis] = 1.0;
                let q = glam::Quat::from_axis_angle(along, deg.to_radians());
                for id in &ids {
                    self.scene.get_mut(*id).unwrap().turn_about(q, b0.center());
                }
            }
            Kind::Size(axis, side) => {
                let grow = amount * side;
                let mut size = b0.size();
                let along = (size[axis] + if snap { grow.round() } else { grow }).max(0.1);
                if self.lock_scale {
                    // Locked: every side follows, keeping the proportions.
                    size *= along / size[axis].max(1e-6);
                } else {
                    size[axis] = along;
                }
                self.scene.resize_selection(size);
                // The opposite face stays where it was; with the lock, the rest keep their
                // centre and the bottom stays on the floor.
                let b = self.scene.selection_aabb();
                let mut shift = b0.center() - b.center();
                shift.z = b0.min.z - b.min.z;
                shift[axis] = if side > 0.0 {
                    b0.min[axis] - b.min[axis]
                } else {
                    b0.max[axis] - b.max[axis]
                };
                for id in &ids {
                    self.scene.get_mut(*id).unwrap().translation += shift;
                }
            }
            Kind::Uniform => {
                let k = (1.0 + amount).max(0.01);
                self.scene.resize_selection(b0.size() * k);
                // Same footprint centre, same floor.
                let b = self.scene.selection_aabb();
                let shift = Vec3::new(
                    b0.center().x - b.center().x,
                    b0.center().y - b.center().y,
                    b0.min.z - b.min.z,
                );
                for id in &ids {
                    self.scene.get_mut(*id).unwrap().translation += shift;
                }
            }
        }
        self.scene.dirty = true;
    }
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
    before: Snapshot,
}

fn draw_box(
    painter: &egui::Painter,
    b: &Aabb,
    stroke: Stroke,
    to_screen: &dyn Fn(Vec3) -> Option<Pos2>,
) {
    if !b.is_valid() {
        return;
    }
    let c = |i: u8| {
        Vec3::new(
            if i & 1 != 0 { b.max.x } else { b.min.x },
            if i & 2 != 0 { b.max.y } else { b.min.y },
            if i & 4 != 0 { b.max.z } else { b.min.z },
        )
    };
    for (a, bb) in [
        (0, 1),
        (1, 3),
        (3, 2),
        (2, 0),
        (4, 5),
        (5, 7),
        (7, 6),
        (6, 4),
        (0, 4),
        (1, 5),
        (2, 6),
        (3, 7),
    ] {
        if let (Some(p), Some(q)) = (to_screen(c(a)), to_screen(c(bb))) {
            painter.line_segment([p, q], stroke);
        }
    }
}

/// The webcam's live picture, fitted in the view, in place of the scene.
fn camera_feed(
    ui: &egui::Ui,
    rect: egui::Rect,
    cam: &crate::webcam::Webcam,
    tex: &mut Option<(u64, egui::TextureHandle)>,
    lang: Lang,
) {
    ui.painter()
        .rect_filled(rect, egui::CornerRadius::ZERO, Color32::BLACK);
    match cam.latest() {
        Some(Ok((n, frame))) => {
            if tex.as_ref().map(|t| t.0) != Some(n) {
                let size = [frame.width() as usize, frame.height() as usize];
                let image = egui::ColorImage::from_rgba_unmultiplied(size, frame.as_raw());
                match tex {
                    Some((shown, t)) => {
                        t.set(image, egui::TextureOptions::LINEAR);
                        *shown = n;
                    }
                    None => {
                        let t =
                            ui.ctx()
                                .load_texture("camera", image, egui::TextureOptions::LINEAR);
                        *tex = Some((n, t));
                    }
                }
            }
            if let Some((_, t)) = tex {
                let size = t.size_vec2();
                let k = (rect.width() / size.x).min(rect.height() / size.y);
                let fit = egui::Rect::from_center_size(rect.center(), size * k);
                let uv = egui::Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
                ui.painter().image(t.id(), fit, uv, Color32::WHITE);
            }
        }
        Some(Err(_)) => {}
        None => {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr(lang, "Starting the camera…"),
                egui::FontId::proportional(18.0),
                style::TEXT_DIM,
            );
        }
    }
    // New frames keep coming while the camera is open.
    ui.ctx().request_repaint_after(Duration::from_millis(33));
}
