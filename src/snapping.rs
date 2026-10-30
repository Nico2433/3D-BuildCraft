//! Bounding-box alignment, independent of the UI and camera.
use crate::mesh::Aabb;

#[derive(Clone, Copy, Debug)]
pub struct Snap {
    pub offset: f32,
    pub value: f32,
    pub target: Aabb,
}

/// Find the nearest pair of minimum, centre or maximum features on one axis.
pub fn snap_axis(moving: &Aabb, targets: &[Aabb], axis: usize, threshold: f32) -> Option<Snap> {
    let features = |b: &Aabb| [b.min[axis], b.center()[axis], b.max[axis]];
    let mut best: Option<Snap> = None;
    for target in targets.iter().filter(|b| b.is_valid()) {
        for source in features(moving) {
            for value in features(target) {
                let offset = value - source;
                if offset.abs() <= threshold && best.is_none_or(|b| offset.abs() < b.offset.abs()) {
                    best = Some(Snap {
                        offset,
                        value,
                        target: *target,
                    });
                }
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;
    fn bounds(min: f32, max: f32) -> Aabb {
        Aabb {
            min: Vec3::splat(min),
            max: Vec3::splat(max),
        }
    }
    #[test]
    fn centre_to_centre() {
        let hit = snap_axis(&bounds(0., 10.), &[bounds(3., 8.)], 0, 0.6).unwrap();
        assert_eq!(hit.offset, 0.5);
        assert_eq!(hit.value, 5.5);
    }
    #[test]
    fn edge_to_edge() {
        assert_eq!(
            snap_axis(&bounds(0., 10.), &[bounds(0.4, 30.)], 0, 0.5)
                .unwrap()
                .offset,
            0.4
        );
    }
    #[test]
    fn edge_to_opposite_edge() {
        assert!(
            (snap_axis(&bounds(0., 10.), &[bounds(10.3, 20.)], 0, 0.5)
                .unwrap()
                .offset
                - 0.3)
                .abs()
                < 1e-5
        );
    }
    #[test]
    fn beyond_threshold() {
        assert!(snap_axis(&bounds(0., 10.), &[bounds(12., 22.)], 0, 0.5).is_none());
    }
}
