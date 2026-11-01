//! Handle drags: gizmo, snapping, resize/move/turn, cut plane.

use super::*;

impl App {
    pub(super) fn rotation_drag_angle(&self, degrees: f32) -> f32 {
        if self.settings.snap {
            (degrees / 5.0).round() * 5.0
        } else {
            degrees
        }
    }

    /// The handles around the selection, in screen points; `None` when nothing is
    /// selected or a tool or the print view is open.
    pub(super) fn gizmo(&self, aspect: f32, rect: egui::Rect) -> Option<gizmo::Gizmo> {
        let splitting = matches!(self.tool, Some(Tool::Cut { .. }));
        if self.scene.selection.is_empty()
            || (self.tool.is_some() && !splitting)
            || self.print.is_some()
        {
            return None;
        }
        let to_screen = |p: Vec3| self.to_screen(p, rect, aspect);
        let drag = self.handle.as_ref().and_then(|h| {
            let kind = h.handle?.kind;
            Some((kind, self.rotation_drag_angle(h.turned)))
        });
        let mut angles = self.rotate_angles;
        if let Some(Tool::Cut { plane, .. }) = &self.tool {
            // Where the plane started the drag, so the handles do not follow it.
            let at_start = self.handle.as_ref().and_then(|h| h.cut).unwrap_or(*plane);
            angles = at_start.rotation.to_array();
            if self.mode == TransformMode::Move {
                // The arrow sits on the plane's edge nearest the bottom of the screen.
                let slab = self.cut_slab(plane);
                let edge = (0..4)
                    .map(|i| (slab[i] + slab[(i + 1) % 4]) * 0.5)
                    .filter_map(|p| Some((p, to_screen(p)?.y)))
                    .max_by(|a, b| a.1.total_cmp(&b.1))
                    .map_or(plane.point(), |e| e.0);
                return Some(gizmo::plane_arrow(edge, plane.normal(), &to_screen));
            }
        }
        // The Split rings turn about the plane's middle.
        let mut around = self.scene.selection_aabb();
        if let Some(Tool::Cut { plane, .. }) = &self.tool {
            around = Aabb {
                min: plane.point(),
                max: plane.point(),
            };
        }
        Some(gizmo::build(
            self.mode,
            &around,
            self.scene.selection_radius(),
            self.camera.eye(),
            angles,
            drag,
            &to_screen,
        ))
    }

    /// The Split tool's plane, if it is open.
    pub(super) fn cut_plane(&self) -> Option<CutPlane> {
        match &self.tool {
            Some(Tool::Cut { plane, .. }) => Some(*plane),
            _ => None,
        }
    }

    /// Keeps the Split plane's middle inside the selection along the plane's normal, so
    /// it cannot be dragged or typed past the object. True when it moved.
    pub(in crate::app) fn clamp_cut_plane(&mut self) -> bool {
        let Some(Tool::Cut { plane, .. }) = &self.tool else {
            return false;
        };
        // The cached selection box keeps this cheap on heavy meshes; it is exact for a
        // level plane and slightly loose for a tilted one.
        let n = plane.normal();
        let b = self.scene.selection_aabb();
        let (mut lo, mut hi) = (f32::INFINITY, f32::NEG_INFINITY);
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { b.min.x } else { b.max.x },
                if i & 2 == 0 { b.min.y } else { b.max.y },
                if i & 4 == 0 { b.min.z } else { b.max.z },
            );
            lo = lo.min(n.dot(c));
            hi = hi.max(n.dot(c));
        }
        let d = n.dot(plane.point());
        if lo > hi || (d.clamp(lo, hi) - d).abs() < 1e-4 {
            return false;
        }
        let target = d.clamp(lo, hi);
        if let Some(Tool::Cut { plane, .. }) = &mut self.tool {
            plane.offset += target - d;
        }
        true
    }

    /// The corners of the square slab drawn for the Split plane, as wide as the rings.
    pub(super) fn cut_slab(&self, plane: &CutPlane) -> [Vec3; 4] {
        let (q, r) = (plane.quat(), self.scene.selection_radius());
        let (u, v, p) = (q * Vec3::X * r, q * Vec3::Y * r, plane.point());
        [p - u - v, p + u - v, p + u + v, p - u + v]
    }

    /// A point of the scene on screen in the view `rect`, if it is in front of the camera.
    pub(super) fn to_screen(&self, p: Vec3, rect: egui::Rect, aspect: f32) -> Option<Pos2> {
        self.camera.project(p, aspect).map(|n| {
            Pos2::new(
                rect.left() + (n.x + 1.0) * 0.5 * rect.width(),
                rect.top() + (1.0 - n.y) * 0.5 * rect.height(),
            )
        })
    }

    /// Where a box appears on screen in the view `rect`: the bounds of its corners in front
    /// of the camera.
    pub(super) fn screen_box(&self, b: &Aabb, rect: egui::Rect, aspect: f32) -> Option<egui::Rect> {
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
    pub(super) fn pick_handle(&mut self, kind: gizmo::Kind) {
        if let Some(f) = kind.field() {
            self.field = f;
        }
    }

    /// Convert eight logical screen pixels at the selection depth into millimetres.
    pub(super) fn snap_movement(
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

    /// Starts a handle drag already `amount` along (`--drag`, for screenshots): `x`, `y`, `z`
    /// move along an axis, `rot-x`, `rot-y`, `rot-z` turn about it.
    pub(in crate::app) fn simulate_drag(&mut self, spec: &str) {
        use gizmo::Kind;
        let Some((name, amount)) = spec
            .split_once(':')
            .and_then(|(n, a)| Some((n, a.parse::<f32>().ok()?)))
        else {
            return;
        };
        let axis = |c: &str| ["x", "y", "z"].iter().position(|a| *a == c);
        let kind = match name.strip_prefix("rot-") {
            Some(a) => axis(a).map(Kind::Turn),
            None => axis(name).map(Kind::Move),
        };
        let Some(kind) = kind else { return };
        let (start, b0) = (self.scene.objects.clone(), self.scene.selection_aabb());
        self.handle = Some(HandleDrag {
            handle: Some(gizmo::Handle {
                kind,
                at: Pos2::ZERO,
                per: egui::Vec2::X,
                pivot: None,
            }),
            plane_z: 0.0,
            from: Pos2::ZERO,
            last: Pos2::ZERO,
            turned: amount,
            start: start.clone(),
            aabb: b0,
            cut: self.cut_plane(),
            before: self.snapshot(),
        });
        self.drag_handle(kind, amount, start, &b0, true);
    }

    /// Applies a handle drag of `amount` (mm, degrees or a size factor) to the objects as
    /// they were when it started.
    pub(super) fn drag_handle(
        &mut self,
        kind: gizmo::Kind,
        amount: f32,
        start: Vec<Object>,
        b0: &Aabb,
        unsnapped: bool,
    ) {
        use gizmo::Kind;
        let snap = self.settings.snap && !unsnapped;
        // The Split tool drags its plane, not the objects.
        if let (Some(Tool::Cut { plane, .. }), Some(mut moved)) =
            (&mut self.tool, self.handle.as_ref().and_then(|h| h.cut))
        {
            match kind {
                Kind::Move(_) => moved.offset += if snap { amount.round() } else { amount },
                Kind::Turn(axis) => {
                    let mut along = Vec3::ZERO;
                    along[axis] = 1.0;
                    let deg = if self.settings.snap {
                        (amount / 5.0).round() * 5.0
                    } else {
                        amount
                    };
                    moved.turn(glam::Quat::from_axis_angle(along, deg.to_radians()));
                }
                _ => {}
            }
            *plane = moved;
            // Clamp before this frame paints the plane, or each drag frame shows it past
            // the object.
            self.clamp_cut_plane();
            return;
        }
        self.scene.objects = start;
        let ids = self.scene.selected_ids();
        match kind {
            Kind::Move(axis) => {
                let d = if snap { amount.round() } else { amount };
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

impl App {
    /// Wheel zooms, right/middle drag pans (left drag on empty space orbits).
    pub(super) fn camera_input(&mut self, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect) {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        let sticker_wheel = matches!(&self.tool, Some(Tool::Texture(t)) if t.is_editing());
        if resp.hovered() && scroll != 0.0 && !sticker_wheel {
            self.glide = None;
            self.camera.zoom(scroll);
        }
        if resp.dragged_by(PointerButton::Secondary) || resp.dragged_by(PointerButton::Middle) {
            let d = resp.drag_delta();
            self.glide = None;
            self.camera.pan(GVec2::new(d.x, d.y), rect.height());
        }
    }

    /// A press on the selection or one of its handles starts a handle drag.
    pub(super) fn begin_drag(
        &mut self,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
        gizmo: Option<&gizmo::Gizmo>,
        box_press: Option<(Pos2, bool)>,
        marquee_modifier: bool,
    ) {
        if self.tool.is_none() && resp.is_pointer_button_down_on() {
            if let Some((pos, true)) = box_press {
                self.marquee = Some(pos);
            }
        }
        if resp.drag_started_by(PointerButton::Primary)
            && matches!(self.tool, None | Some(Tool::Cut { .. }))
            && self.marquee.is_none()
            && !marquee_modifier
        {
            let at = resp.interact_pointer_pos();
            let grabbed = at.zip(gizmo).and_then(|(p, g)| g.grab(p));
            // Pressed on a selected object (and no handle): drag it on the floor.
            let on_selection = at
                .filter(|_| grabbed.is_none() && self.tool.is_none())
                .and_then(|p| {
                    let (o, d) = self.camera.ray(to_ndc(rect, p), aspect);
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
                    cut: self.cut_plane(),
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
                    cut: self.cut_plane(),
                    before,
                });
            }
        }
    }

    /// Applies the running handle drag for this frame; returns the snap guides to draw.
    pub(super) fn update_handle_drag(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) -> Vec<(Vec3, Vec3)> {
        let mut snap_guides = Vec::new();
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
                self.drag_handle(
                    handle.kind,
                    amount,
                    start,
                    &b0,
                    ui.input(|i| i.modifiers.alt),
                );
            } else {
                // The grabbed point follows the pointer on its horizontal plane.
                let on_plane = |q: Pos2| {
                    let (o, d) = self.camera.ray(to_ndc(rect, q), aspect);
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
            if self.tool.is_none() && !ui.input(|i| i.modifiers.alt) {
                snap_guides = self.snap_movement(axes, rect, aspect);
            }
        }
        snap_guides
    }

    /// Dragging over the selection places the emboss; shift turns it.
    pub(super) fn emboss_place(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) {
        let shift = ui.input(|i| i.modifiers.shift);
        let (o, d) = match resp.interact_pointer_pos() {
            Some(pos) => self.camera.ray(to_ndc(rect, pos), aspect),
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
                params.angle =
                    (params.angle + resp.drag_delta().x * 0.5 + 180.0).rem_euclid(360.0) - 180.0;
            } else if let (Some((hit_id, p)), Some((id, mesh, matrix))) = (hit, target) {
                if hit_id == id {
                    params.offset =
                        crate::emboss::offset_at_placed(&mesh, matrix, params.projection, p);
                }
            }
        }
    }

    /// Selects what the selection box covers once the button is released.
    pub(super) fn finish_marquee(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) {
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
    }

    /// One undo step for the whole handle drag, none when nothing changed.
    pub(super) fn finish_handle_drag(&mut self) {
        // One undo step for the whole handle drag, none when nothing changed.
        if let Some(h) = self.handle.take() {
            if let Some(gizmo::Handle {
                kind: gizmo::Kind::Turn(axis),
                ..
            }) = h.handle.filter(|_| self.tool.is_none())
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

    /// The measure marker takes the place of the cursor.
    pub(super) fn measure_hover(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
    ) {
        if matches!(self.tool, Some(Tool::Measure { .. })) {
            // The marker takes the place of the cursor.
            let spot = resp.hover_pos().and_then(|p| {
                let (o, d) = self.camera.ray(to_ndc(rect, p), aspect);
                self.measure_spot(o, d)
            });
            if let Some(Tool::Measure { hover, points }) = &mut self.tool {
                if spot.is_some() || ui.ctx().pointer_hover_pos().is_some() {
                    *hover = spot;
                }
                if resp.hovered() && points.len() < 2 {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::None);
                }
            }
        }
    }

    /// A click on a handle picks it; otherwise it acts on the active tool or selects.
    pub(super) fn click(
        &mut self,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
        gizmo: Option<&gizmo::Gizmo>,
    ) {
        // A click on a handle picks it (and its field) instead of selecting.
        let clicked_handle = resp
            .clicked()
            .then(|| resp.interact_pointer_pos().zip(gizmo))
            .flatten()
            .and_then(|(p, g)| g.grab(p).filter(|_| self.tool.is_none()));
        if let Some(h) = clicked_handle {
            self.pick_handle(h.kind);
        } else if resp.clicked() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let (o, d) = self.camera.ray(to_ndc(rect, pos), aspect);
                let hit = self.pick(o, d);
                let spot = self.measure_spot(o, d);
                let face = matches!(self.tool, Some(Tool::Paint { .. } | Tool::Material { .. }))
                    .then(|| self.pick_face(o, d))
                    .flatten();
                match &mut self.tool {
                    Some(Tool::Measure { points, .. }) => {
                        if let Some(p) = spot {
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
                    Some(Tool::Material { dropper, .. }) => {
                        if let Some((id, tri)) = face.filter(|_| *dropper) {
                            self.pick_material(id, tri);
                        }
                    }
                    Some(
                        Tool::Settle { .. }
                        | Tool::Cut { .. }
                        | Tool::Simplify { .. }
                        | Tool::Smooth { .. }
                        | Tool::Hollow { .. }
                        | Tool::Emboss { .. }
                        | Tool::Custom { .. }
                        | Tool::Image { .. }
                        | Tool::Texture(_)
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
    }

    /// A double click centres the view on the object, as in the original.
    pub(super) fn double_click(&mut self, resp: &egui::Response, rect: egui::Rect, aspect: f32) {
        if resp.double_clicked() && self.tool.is_none() {
            if let Some(pos) = resp.interact_pointer_pos() {
                let (o, d) = self.camera.ray(to_ndc(rect, pos), aspect);
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
    }
}
