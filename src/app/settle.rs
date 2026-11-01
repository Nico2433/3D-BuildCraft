//! Transform-only damped rigid-body motion for the interactive Settle tool.
use super::{icons, Tool};
use crate::{mesh::Aabb, scene::Object};
use eframe::egui;
use glam::{Quat, Vec2, Vec3};

#[derive(Default)]
pub(super) struct Body {
    pub id: u64,
    velocity: f32,
    angular: Vec3,
    pub grab: Option<(Vec3, Vec3, Vec3)>, // local point, world target, plane normal
    sleeping: bool,
}

impl Body {
    pub fn new(id: u64) -> Self {
        Self {
            id,
            ..Self::default()
        }
    }

    pub fn wake(&mut self) {
        self.sleeping = false;
    }

    pub fn step(&mut self, object: &mut Object, obstacles: &[Aabb], dt: f32) -> bool {
        if self.sleeping && self.grab.is_none() {
            return false;
        }
        // Substeps keep collisions and pendulum integration independent of frame rate.
        let local_center = object
            .mesh
            .centroid()
            .unwrap_or_else(|| object.mesh.aabb().center());
        let steps = (dt / 0.008).ceil().max(1.0) as usize;
        let h = dt / steps as f32;
        for _ in 0..steps {
            let center = object.matrix().transform_point3(local_center);
            if let Some((local, target, _)) = self.grab {
                let pivot = object.matrix().transform_point3(local);
                object.translation += target - pivot;
                let arm = center - pivot;
                self.angular += arm.cross(-Vec3::Z) * (1200.0 / arm.length_squared().max(1.0)) * h;
                self.angular *= (-5.0 * h).exp();
                object.turn_about(Quat::from_scaled_axis(self.angular * h), target);
                self.velocity = 0.0;
            } else {
                let previous = object.world_aabb();
                self.velocity -= 980.0 * h;
                object.translation.z += self.velocity * h;
                object.turn_about(Quat::from_scaled_axis(self.angular * h), center);
                let b = object.world_aabb();
                let floor = obstacles
                    .iter()
                    .filter(|o| {
                        o.min.x < b.max.x
                            && o.max.x > b.min.x
                            && o.min.y < b.max.y
                            && o.max.y > b.min.y
                            && o.max.z <= previous.min.z + 0.05
                    })
                    .map(|o| o.max.z)
                    .fold(0.0_f32, f32::max);
                if b.min.z <= floor + 0.001 {
                    object.translation.z += floor - b.min.z;
                    self.velocity = 0.0;
                    let matrix = object.matrix();
                    let points: Vec<_> = object
                        .mesh
                        .positions
                        .iter()
                        .map(|p| matrix.transform_point3(*p))
                        .filter(|p| p.z <= floor + object.mesh.aabb().size().max_element() * 0.002)
                        .collect();
                    let com = matrix.transform_point3(local_center);
                    let mut contacts = hull(points.iter().map(|p| p.truncate()).collect());
                    if floor > 0.0 {
                        if let Some(box_top) = obstacles.iter().find(|o| {
                            (o.max.z - floor).abs() < 1e-4
                                && o.min.x < b.max.x
                                && o.max.x > b.min.x
                                && o.min.y < b.max.y
                                && o.max.y > b.min.y
                        }) {
                            contacts = clip_support(contacts, box_top);
                        }
                    }
                    if let Some(edge) = support(&contacts, com.truncate()) {
                        let pivot = edge.extend(floor);
                        let arm = com - pivot;
                        let mut torque = arm.cross(-Vec3::Z);
                        // An exactly balanced edge is unstable; choose a deterministic side.
                        if torque.length_squared() < 1e-8 {
                            torque = Vec3::X * arm.length().max(1.0) * 0.1;
                        }
                        self.angular += torque * (1800.0 / arm.length_squared().max(1.0)) * h;
                        object.turn_about(Quat::from_scaled_axis(self.angular * h), pivot);
                        object.translation.z += (floor - object.world_aabb().min.z).max(0.0);
                    } else {
                        self.angular *= (-35.0 * h).exp();
                        if self.angular.length() < 0.05 {
                            // Remove the final tiny contact tolerance tilt on a planar face.
                            let down = object
                                .mesh
                                .triangle_positions()
                                .map(|triangle| {
                                    let [a, b, c] =
                                        triangle.map(|p| object.matrix().transform_point3(p));
                                    crate::mesh::Mesh::face_normal(a, b, c)
                                })
                                .filter(|n| n.dot(-Vec3::Z) > 0.9999)
                                .max_by(|a, b| a.dot(-Vec3::Z).total_cmp(&b.dot(-Vec3::Z)));
                            if let Some(normal) = down {
                                object.turn_about(Quat::from_rotation_arc(normal, -Vec3::Z), com);
                                object.translation.z += floor - object.world_aabb().min.z;
                            }
                            self.angular = Vec3::ZERO;
                            self.sleeping = true;
                        }
                    }
                    self.angular *= (-8.0 * h).exp();
                } else {
                    self.angular *= (-1.5 * h).exp();
                }
            }
        }
        !self.sleeping || self.grab.is_some()
    }
}

/// Clip the contact polygon to the actual supporting box top.
fn clip_support(mut points: Vec<Vec2>, bounds: &Aabb) -> Vec<Vec2> {
    for (normal, limit) in [
        (Vec2::X, bounds.max.x),
        (-Vec2::X, -bounds.min.x),
        (Vec2::Y, bounds.max.y),
        (-Vec2::Y, -bounds.min.y),
    ] {
        let mut clipped = Vec::new();
        for i in 0..points.len() {
            let a = points[i];
            let b = points[(i + 1) % points.len()];
            let da = a.dot(normal) - limit;
            let db = b.dot(normal) - limit;
            if da <= 0.0 {
                clipped.push(a);
            }
            if (da <= 0.0) != (db <= 0.0) {
                clipped.push(a + (b - a) * (da / (da - db)));
            }
        }
        points = clipped;
    }
    hull(points)
}

fn hull(mut points: Vec<Vec2>) -> Vec<Vec2> {
    points.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    points.dedup_by(|a, b| a.distance_squared(*b) < 1e-8);
    if points.len() < 3 {
        return points;
    }
    let mut out: Vec<Vec2> = Vec::new();
    for p in &points {
        while out.len() >= 2
            && (out[out.len() - 1] - out[out.len() - 2]).perp_dot(*p - out[out.len() - 1]) <= 0.0
        {
            out.pop();
        }
        out.push(*p);
    }
    let lower = out.len();
    for p in points.iter().rev().skip(1) {
        while out.len() > lower
            && (out[out.len() - 1] - out[out.len() - 2]).perp_dot(*p - out[out.len() - 1]) <= 0.0
        {
            out.pop();
        }
        out.push(*p);
    }
    out.pop();
    out
}

/// Closest support edge when the projected centre lies outside the contact hull.
fn support(points: &[Vec2], center: Vec2) -> Option<Vec2> {
    if points.len() >= 3
        && (0..points.len()).all(|i| {
            (points[(i + 1) % points.len()] - points[i]).perp_dot(center - points[i]) >= -1e-4
        })
    {
        return None;
    }
    (0..points.len())
        .map(|i| {
            let a = points[i];
            let d = points[(i + 1) % points.len()] - a;
            a + d * ((center - a).dot(d) / d.length_squared().max(1e-10)).clamp(0.0, 1.0)
        })
        .min_by(|a, b| {
            a.distance_squared(center)
                .total_cmp(&b.distance_squared(center))
        })
}

impl super::App {
    pub(super) fn update_settle(
        &mut self,
        ctx: &egui::Context,
        ui: &egui::Ui,
        resp: &egui::Response,
        rect: egui::Rect,
        aspect: f32,
        box_press: Option<(egui::Pos2, bool)>,
    ) {
        let to_ndc = |p: egui::Pos2| {
            Vec2::new(
                (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
                1.0 - (p.y - rect.top()) / rect.height() * 2.0,
            )
        };
        // Settle owns its grab and velocities independently of transform handles.
        let settle_hit = if matches!(self.tool, Some(Tool::Settle { .. })) {
            resp.hover_pos()
                .and_then(|p| {
                    let (o, d) = self.camera.ray(to_ndc(p), aspect);
                    self.pick(o, d)
                })
                .filter(|(id, _)| self.scene.selection.contains(id))
        } else {
            None
        };
        if matches!(self.tool, Some(Tool::Settle { .. })) {
            let press_hit = box_press.and_then(|(p, _)| {
                let (o, d) = self.camera.ray(to_ndc(p), aspect);
                self.pick(o, d).map(|hit| (hit, d))
            });
            let dt = ui.input(|i| i.stable_dt).clamp(0.001, 0.05);
            let down = ui.input(|i| i.pointer.primary_down());
            if let Some(Tool::Settle { bodies, .. }) = &mut self.tool {
                let mut moving = false;
                for body in bodies {
                    if let Some(((id, hit), normal)) = press_hit {
                        if id == body.id {
                            if let Some(object) = self.scene.get(id) {
                                body.grab = Some((
                                    object.matrix().inverse().transform_point3(hit),
                                    hit,
                                    if normal.truncate().length_squared() > 0.01 {
                                        Vec3::new(normal.x, normal.y, 0.0).normalize()
                                    } else {
                                        Vec3::Z
                                    },
                                ));
                                body.wake();
                            }
                        }
                    }
                    if !down {
                        body.grab = None;
                    }
                    if let Some((_, target, normal)) = &mut body.grab {
                        if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                            let (o, d) = self.camera.ray(to_ndc(pos), aspect);
                            let denominator = d.dot(*normal);
                            if denominator.abs() > 1e-5 {
                                let t = (*target - o).dot(*normal) / denominator;
                                if t > 0.0 {
                                    *target = o + d * t;
                                }
                            }
                        }
                    }
                    let obstacles: Vec<_> = self
                        .scene
                        .objects
                        .iter()
                        .filter(|o| o.id != body.id)
                        .map(Object::world_aabb)
                        .collect();
                    if let Some(object) = self.scene.get_mut(body.id) {
                        if let Some((local, target, _)) = &mut body.grab {
                            // Keep the suspended body above the floor and boxes below it.
                            let b = object.world_aabb();
                            let grab = object.matrix().transform_point3(*local);
                            let shifted = *target - grab;
                            let floor = obstacles
                                .iter()
                                .filter(|o| {
                                    o.min.x < b.max.x + shifted.x
                                        && o.max.x > b.min.x + shifted.x
                                        && o.min.y < b.max.y + shifted.y
                                        && o.max.y > b.min.y + shifted.y
                                        && o.max.z <= b.max.z + shifted.z
                                })
                                .map(|o| o.max.z)
                                .fold(0.0_f32, f32::max);
                            target.z = target.z.max(floor + grab.z - b.min.z + 0.1);
                        }
                        moving |= body.step(object, &obstacles, dt);
                    }
                }
                if moving {
                    self.scene.dirty = true;
                    ctx.request_repaint();
                }
            }
            let grabbing = matches!(&self.tool, Some(Tool::Settle { bodies, .. }) if bodies.iter().any(|b| b.grab.is_some()));
            if grabbing || settle_hit.is_some() {
                icons::hand_cursor(ctx, grabbing);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use crate::{primitives::Primitive, scene::DEFAULT_COLOR};
    #[test]
    fn grab_point_stays_attached_while_body_swings() {
        let mut object = Object::new("Box", Primitive::Cube.build(), DEFAULT_COLOR);
        let local = object.mesh.aabb().max;
        let target = Vec3::new(20.0, 10.0, 120.0);
        let mut body = Body::new(0);
        body.grab = Some((local, target, Vec3::Y));
        for _ in 0..120 {
            body.step(&mut object, &[], 1.0 / 120.0);
        }
        assert!(object.matrix().transform_point3(local).distance(target) < 1e-3);
        assert!(object.rotation.length() > 1.0);
    }

    #[test]
    fn falls_with_acceleration_and_stops_on_box_top() {
        let mut object = Object::new("Box", Primitive::Cube.build(), DEFAULT_COLOR);
        object.translation.z = 150.0;
        let mut body = Body::new(0);
        body.step(&mut object, &[], 0.02);
        let first = object.translation.z;
        body.step(&mut object, &[], 0.02);
        assert!(first - object.translation.z > 150.0 - first);
        let support = Aabb {
            min: Vec3::new(-100.0, -100.0, 0.0),
            max: Vec3::new(100.0, 100.0, 30.0),
        };
        for _ in 0..240 {
            body.step(&mut object, &[support], 1.0 / 120.0);
        }
        assert!(body.sleeping);
        assert!((object.world_aabb().min.z - 30.0).abs() < 1e-3);
    }

    #[test]
    fn falls_and_topples_without_penetrating_floor() {
        for angle in [0.0, 35.0, 45.0] {
            let mut object = Object::new("Box", Primitive::Cube.build(), DEFAULT_COLOR);
            object.rotation.x = angle;
            object.translation.z = 100.0;
            let mesh = object.mesh.clone();
            let mut body = Body::new(0);
            for _ in 0..240 {
                body.step(&mut object, &[], 1.0 / 120.0);
                assert!(object.world_aabb().min.z >= -1e-4);
            }
            assert!(body.sleeping, "angle {angle}: {:?}", object.rotation);
            assert!(object.world_aabb().min.z.abs() < 0.001);
            assert!(std::sync::Arc::ptr_eq(&mesh, &object.mesh));
            let size = object.world_aabb().size();
            assert!((size.z - mesh.aabb().size().z).abs() < 0.1);
        }
    }
}
