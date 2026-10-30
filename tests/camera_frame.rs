use buildcraft3d::camera::OrbitCamera;
use buildcraft3d::mesh::Aabb;
use glam::{Vec2, Vec3};

fn corners(b: &Aabb) -> Vec<Vec3> {
    (0..8)
        .map(|i| {
            Vec3::new(
                if i & 1 != 0 { b.max.x } else { b.min.x },
                if i & 2 != 0 { b.max.y } else { b.min.y },
                if i & 4 != 0 { b.max.z } else { b.min.z },
            )
        })
        .collect()
}

fn assert_framed(b: &Aabb, aspect: f32) {
    let mut cam = OrbitCamera::default();
    cam.frame(b, aspect);
    assert!((cam.target - b.center()).length() < 1e-4);
    for c in corners(b) {
        let n = cam
            .project(c, aspect)
            .unwrap_or_else(|| panic!("corner {c:?} behind the camera at aspect {aspect}"));
        assert!(
            n.x.abs() <= 1.0 && n.y.abs() <= 1.0,
            "corner {c:?} at aspect {aspect} projects to {n:?}"
        );
    }
}

fn big() -> Aabb {
    Aabb {
        min: Vec3::new(-50.0, -20.0, 0.0),
        max: Vec3::new(50.0, 20.0, 80.0),
    }
}

#[test]
fn frame_fits_all_corners_for_square_and_wide_viewports() {
    for aspect in [1.0, 2.0] {
        assert_framed(&big(), aspect);
        assert_framed(
            &Aabb {
                min: Vec3::new(500.0, 500.0, 0.0),
                max: Vec3::new(501.0, 501.0, 1.0),
            },
            aspect,
        );
    }
}

#[test]
fn frame_fits_all_corners_for_a_tall_viewport() {
    assert_framed(&big(), 0.5);
}

#[test]
fn frame_centres_the_target_on_the_box() {
    let mut cam = OrbitCamera::default();
    cam.frame(&big(), 1.0);
    assert_eq!(cam.target, Vec3::new(0.0, 0.0, 40.0));
    let n = cam.project(cam.target, 1.0).unwrap();
    assert!(n.length() < 1e-4, "{n:?}");
}

#[test]
fn frame_with_an_empty_box_leaves_the_camera_unchanged() {
    let mut cam = OrbitCamera::default();
    let before = cam;
    cam.frame(&Aabb::empty(), 1.0);
    assert_eq!(cam.target, before.target);
    assert_eq!(cam.distance, before.distance);
    assert_eq!(cam.yaw, before.yaw);
    assert_eq!(cam.pitch, before.pitch);
}

#[test]
fn zoom_clamps_the_distance() {
    let mut cam = OrbitCamera::default();
    cam.zoom(1e6);
    assert_eq!(cam.distance, 1.0);
    cam.zoom(f32::MAX / 1e6);
    assert_eq!(cam.distance, 1.0);
    cam.zoom(-1e6);
    assert_eq!(cam.distance, 20_000.0);
}

#[test]
fn scrolling_up_moves_closer_and_down_moves_away() {
    let mut cam = OrbitCamera::default();
    let d = cam.distance;
    cam.zoom(0.0);
    assert_eq!(cam.distance, d);
    cam.zoom(100.0);
    assert!(cam.distance < d);
    let closer = cam.distance;
    cam.zoom(-100.0);
    assert!((cam.distance - d).abs() < 1e-2 && cam.distance > closer);
}

#[test]
fn orbit_clamps_pitch_and_keeps_the_target() {
    let mut cam = OrbitCamera::default();
    let t = cam.target;
    cam.orbit(Vec2::new(0.0, 1e6));
    assert_eq!(cam.pitch, 1.5);
    cam.orbit(Vec2::new(0.0, -1e6));
    assert_eq!(cam.pitch, -1.5);
    assert_eq!(cam.target, t);
    // The eye stays `distance` away from the target.
    assert!(((cam.eye() - t).length() - cam.distance).abs() < 1e-2);
}

#[test]
fn pan_moves_the_target_in_the_view_plane() {
    let mut cam = OrbitCamera::default();
    let before = cam.target;
    let fwd = (cam.target - cam.eye()).normalize();
    cam.pan(Vec2::new(30.0, -20.0), 600.0);
    let d = cam.target - before;
    assert!(d.length() > 0.1);
    assert!(
        d.dot(fwd).abs() < 1e-3,
        "pan must not move along the view direction"
    );
}

#[test]
fn points_behind_the_camera_do_not_project() {
    let cam = OrbitCamera::default();
    let behind = cam.eye() + (cam.eye() - cam.target);
    assert!(cam.project(behind, 1.0).is_none());
}
