//! Immutable local-space BVHs, built on a worker and shared by mesh identity.
use super::*;
use std::sync::{mpsc, Arc, Weak};

pub(super) struct Cache {
    entries: Vec<(Weak<Mesh>, Node)>,
    pending: Option<(Weak<Mesh>, mpsc::Receiver<Node>)>,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            pending: None,
        }
    }

    pub fn release(&mut self) {
        let bytes: usize = self.entries.iter().map(|(_, node)| node.heap_bytes()).sum();
        log::info!(
            "power: releasing {} picking BVHs, {} heap bytes",
            self.entries.len(),
            bytes
        );
        *self = Self::new();
    }

    pub fn prepare(&mut self, objects: &[Object], ctx: &egui::Context) -> bool {
        let mut completed = false;
        self.entries.retain(|(mesh, _)| {
            objects
                .iter()
                .any(|o| mesh.ptr_eq(&Arc::downgrade(&o.mesh)))
        });
        if let Some((mesh, rx)) = &self.pending {
            match rx.try_recv() {
                Ok(node) => {
                    self.entries.push((mesh.clone(), node));
                    self.pending = None;
                    completed = true;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.pending = None,
                Err(mpsc::TryRecvError::Empty) => return false,
            }
        }
        if let Some(object) = objects.iter().find(|o| {
            !self
                .entries
                .iter()
                .any(|(mesh, _)| mesh.ptr_eq(&Arc::downgrade(&o.mesh)))
        }) {
            let mesh = object.mesh.clone();
            let (tx, rx) = mpsc::channel();
            self.pending = Some((Arc::downgrade(&mesh), rx));
            let ctx = ctx.clone();
            std::thread::spawn(move || {
                let node = Node::build_all(&mesh);
                if tx.send(node).is_ok() {
                    ctx.request_repaint();
                }
            });
        }
        completed
    }

    pub fn hit(&self, mesh: &Arc<Mesh>, origin: Vec3, dir: Vec3) -> Option<(usize, f32)> {
        let mut best = None;
        if let Some((_, node)) = self
            .entries
            .iter()
            .find(|(weak, _)| weak.ptr_eq(&Arc::downgrade(mesh)))
        {
            node.hit(mesh, origin, dir, &mut best);
        } else {
            // Preserve exact click/paint behavior while the worker builds the BVH.
            for i in 0..mesh.triangles.len() {
                test_triangle(mesh, i, origin, dir, &mut best);
            }
        }
        best
    }
}

struct Node {
    bounds: Aabb,
    triangles: Vec<usize>,
    children: Option<Box<[Node; 2]>>,
}

impl Node {
    fn heap_bytes(&self) -> usize {
        self.triangles.capacity() * std::mem::size_of::<usize>()
            + self.children.as_ref().map_or(0, |children| {
                std::mem::size_of::<[Node; 2]>()
                    + children.iter().map(Node::heap_bytes).sum::<usize>()
            })
    }

    fn build_all(mesh: &Mesh) -> Self {
        let boxes: Vec<Aabb> = mesh
            .triangles
            .iter()
            .map(|t| {
                let mut bounds = Aabb::empty();
                for v in t {
                    bounds.grow(mesh.positions[*v as usize]);
                }
                bounds
            })
            .collect();
        Self::build(&boxes, (0..mesh.triangles.len()).collect())
    }

    /// `boxes` holds each triangle's bounds, so the sort does not recompute them.
    fn build(boxes: &[Aabb], mut triangles: Vec<usize>) -> Self {
        let bounds = triangles
            .iter()
            .fold(Aabb::empty(), |b, &i| b.union(boxes[i]));
        if triangles.len() <= 16 {
            // split_off keeps the parent's capacity in the left branch.
            triangles.shrink_to_fit();
            return Self {
                bounds,
                triangles,
                children: None,
            };
        }
        let size = bounds.size();
        let axis = if size.x >= size.y && size.x >= size.z {
            0
        } else if size.y >= size.z {
            1
        } else {
            2
        };
        let middle = triangles.len() / 2;
        triangles.select_nth_unstable_by(middle, |&a, &b| {
            boxes[a].center()[axis].total_cmp(&boxes[b].center()[axis])
        });
        let right = triangles.split_off(middle);
        Self {
            bounds,
            triangles: Vec::new(),
            children: Some(Box::new([
                Self::build(boxes, triangles),
                Self::build(boxes, right),
            ])),
        }
    }

    fn hit(&self, mesh: &Mesh, origin: Vec3, dir: Vec3, best: &mut Option<(usize, f32)>) {
        if !ray_aabb(origin, dir, &self.bounds) {
            return;
        }
        if let Some(children) = &self.children {
            for child in children.iter() {
                child.hit(mesh, origin, dir, best);
            }
        } else {
            for &i in &self.triangles {
                test_triangle(mesh, i, origin, dir, best);
            }
        }
    }
}

fn test_triangle(mesh: &Mesh, i: usize, origin: Vec3, dir: Vec3, best: &mut Option<(usize, f32)>) {
    let [a, b, c] = mesh.triangles[i].map(|v| mesh.positions[v as usize]);
    if let Some(t) = ray_triangle(origin, dir, a, b, c) {
        if best.is_none_or(|(old_i, old_t)| t < old_t || (t == old_t && i < old_i)) {
            *best = Some((i, t));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bvh_matches_brute_force() {
        let mut mesh = Mesh::default();
        for i in 0..100 {
            let x = i as f32;
            let n = mesh.positions.len() as u32;
            mesh.positions.extend([
                Vec3::new(x, 0.0, 0.0),
                Vec3::new(x + 0.8, 0.0, 0.0),
                Vec3::new(x, 0.8, 0.0),
            ]);
            mesh.triangles.push([n, n + 1, n + 2]);
        }
        let node = Node::build_all(&mesh);
        for x in -1..202 {
            let origin = Vec3::new(x as f32 * 0.5, 0.2, 5.0);
            let dir = Vec3::new(0.0, 0.0, -2.0);
            let mut expected = None;
            for i in 0..mesh.triangles.len() {
                test_triangle(&mesh, i, origin, dir, &mut expected);
            }
            let mut actual = None;
            node.hit(&mesh, origin, dir, &mut actual);
            assert_eq!(actual, expected);
        }
    }
}

#[cfg(test)]
mod bench {
    use super::*;

    /// `cargo test --offline --release -j 4 -- --ignored --nocapture bvh_build_time`
    #[test]
    #[ignore]
    fn bvh_build_time() {
        let mesh = crate::primitives::sphere(10.0, 400, 400);
        let t = std::time::Instant::now();
        let node = Node::build_all(&mesh);
        println!(
            "BENCH bvh_build {} triangles: {:.1} ms",
            mesh.triangles.len(),
            t.elapsed().as_secs_f64() * 1e3
        );
        let t = std::time::Instant::now();
        let mut n = 0;
        for i in 0..2000 {
            let x = (i % 100) as f32 * 0.2 - 10.0;
            let y = (i / 100) as f32 * 0.5 - 5.0;
            let mut best = None;
            node.hit(&mesh, Vec3::new(x, y, 30.0), -Vec3::Z, &mut best);
            n += best.is_some() as usize;
        }
        println!(
            "BENCH bvh_hit 2000 rays ({n} hits): {:.1} ms",
            t.elapsed().as_secs_f64() * 1e3
        );
    }
}
