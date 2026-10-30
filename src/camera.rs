//! Orbit camera around a target point, Z up.

use crate::mesh::Aabb;
use glam::{Mat4, Vec2, Vec3, Vec4Swizzles};

#[derive(Clone, Copy, Debug)]
pub struct OrbitCamera {
    pub target: Vec3,
    /// Radians around Z, measured from +X.
    pub yaw: f32,
    /// Radians above the floor plane.
    pub pitch: f32,
    pub distance: f32,
    pub fov_y: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::new(0.0, 0.0, 20.0),
            yaw: (-90.0f32).to_radians(),
            pitch: 30f32.to_radians(),
            distance: 260.0,
            fov_y: 40f32.to_radians(),
        }
    }
}

impl OrbitCamera {
    pub fn eye(&self) -> Vec3 {
        let (sp, cp) = self.pitch.sin_cos();
        let (sy, cy) = self.yaw.sin_cos();
        self.target + self.distance * Vec3::new(cp * cy, cp * sy, sp)
    }

    pub fn view(&self) -> Mat4 {
        Mat4::look_at_rh(self.eye(), self.target, Vec3::Z)
    }

    pub fn proj(&self, aspect: f32) -> Mat4 {
        let near = (self.distance * 0.01).max(0.05);
        Mat4::perspective_rh(self.fov_y, aspect.max(1e-3), near, self.distance * 100.0)
    }

    pub fn view_proj(&self, aspect: f32) -> Mat4 {
        self.proj(aspect) * self.view()
    }

    pub fn orbit(&mut self, delta: Vec2) {
        self.yaw -= delta.x * 0.008;
        self.pitch = (self.pitch + delta.y * 0.008).clamp(-1.5, 1.5);
    }

    /// Moves the target in the view plane so the scene follows the pointer.
    pub fn pan(&mut self, delta: Vec2, viewport_height: f32) {
        let world_per_px =
            2.0 * self.distance * (self.fov_y * 0.5).tan() / viewport_height.max(1.0);
        let fwd = (self.target - self.eye()).normalize();
        let right = fwd.cross(Vec3::Z).normalize_or_zero();
        let up = right.cross(fwd);
        self.target += (-right * delta.x + up * delta.y) * world_per_px;
    }

    pub fn zoom(&mut self, scroll: f32) {
        self.distance = (self.distance * (-scroll * 0.0015).exp()).clamp(1.0, 20_000.0);
    }

    /// Centres the box and moves back until its bounding sphere fits the narrower of the
    /// vertical and horizontal fields of view.
    pub fn frame(&mut self, b: &Aabb, aspect: f32) {
        if !b.is_valid() {
            return;
        }
        self.target = b.center();
        let r = (b.size().length() * 0.5).max(5.0);
        let half_h = ((self.fov_y * 0.5).tan() * aspect).atan();
        // Room around the model, as the original leaves.
        self.distance = r / (self.fov_y * 0.5).min(half_h).sin() * 1.9;
    }

    /// Ray through a point given in normalised device coordinates (-1..1, y up).
    pub fn ray(&self, ndc: Vec2, aspect: f32) -> (Vec3, Vec3) {
        let inv = self.view_proj(aspect).inverse();
        let near = inv * glam::Vec4::new(ndc.x, ndc.y, 0.0, 1.0);
        let far = inv * glam::Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let (n, f) = (near.xyz() / near.w, far.xyz() / far.w);
        (n, (f - n).normalize())
    }

    /// World point to NDC; `None` when behind the camera.
    pub fn project(&self, p: Vec3, aspect: f32) -> Option<Vec2> {
        let c = self.view_proj(aspect) * p.extend(1.0);
        (c.w > 1e-6).then(|| c.xy() / c.w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn centre_ray_hits_the_target() {
        let cam = OrbitCamera::default();
        let (o, d) = cam.ray(Vec2::ZERO, 1.5);
        let t = (cam.target - o).dot(d);
        assert!((o + d * t - cam.target).length() < 1e-2);
    }

    #[test]
    fn project_inverts_ray() {
        let cam = OrbitCamera::default();
        let p = Vec3::new(10.0, -5.0, 3.0);
        let ndc = cam.project(p, 1.5).unwrap();
        let (o, d) = cam.ray(ndc, 1.5);
        let closest = o + d * (p - o).dot(d);
        assert!((closest - p).length() < 1e-2);
    }
}
