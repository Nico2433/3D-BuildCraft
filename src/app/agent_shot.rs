//! The screenshot tool's own views: with `view`, `azimuth`, `elevation`, `focus`,
//! `isolate` or `size` it draws the scene off screen with the CPU renderer of
//! `crate::thumbnail` instead of capturing the window. It never moves the user's camera
//! or changes the selection or the scene, and works while the window is minimized.
//! Schema: `crate::mcp::tools`.

use super::*;
use crate::thumbnail::{self, View};
use glam::Mat4;
use serde_json::Value;
use std::ops::RangeInclusive;
use std::sync::Arc;

/// Picture size when `size` is not given, in pixels.
const DEFAULT_SIZE: [u32; 2] = [800, 600];
/// Each side of a picture is clamped to this, in pixels.
const SIDES: RangeInclusive<u32> = 64..=2048;
/// Arguments that ask for an off-screen picture.
const OPTIONS: [&str; 6] = ["view", "azimuth", "elevation", "focus", "isolate", "size"];
/// Named views as (azimuth, elevation) in degrees. Azimuth 0 looks from the front (the
/// -Y side, as the app's starting camera) and grows clockwise seen from above, so 90 is
/// the right (+X) side.
const VIEWS: [(&str, f32, f32); 7] = [
    ("front", 0.0, 0.0),
    ("back", 180.0, 0.0),
    ("left", 270.0, 0.0),
    ("right", 90.0, 0.0),
    ("top", 0.0, 90.0),
    ("bottom", 0.0, -90.0),
    // atan(1/√2): the three axes look equally long.
    ("iso", 45.0, 35.264),
];

/// An off-screen picture to draw: the objects' meshes with their placements and colours.
pub(super) struct Shot {
    parts: Vec<(Arc<Mesh>, Mat4, Rgba)>,
    view: View,
}

impl Shot {
    /// Reads the screenshot arguments; `Ok(None)` when none of the options is given, for
    /// the window capture.
    pub(super) fn parse(scene: &Scene, a: &Value) -> Result<Option<Shot>, String> {
        if !OPTIONS.iter().any(|k| !a[k].is_null()) {
            return Ok(None);
        }
        if scene.objects.is_empty() {
            return Err("the scene is empty; there is nothing to draw".into());
        }
        let (azimuth, elevation) = angles(a)?;
        let focus = focus(scene, &a["focus"])?;
        let isolate = match &a["isolate"] {
            Value::Null => false,
            v => v.as_bool().ok_or("isolate must be true or false")?,
        };
        let [width, height] = size(&a["size"])?;
        let focused = |o: &&Object| focus.as_ref().is_none_or(|ids| ids.contains(&o.id));
        let frame = scene
            .objects
            .iter()
            .filter(focused)
            .map(Object::world_aabb)
            .fold(Aabb::empty(), Aabb::union);
        let parts = scene
            .objects
            .iter()
            .filter(|o| !isolate || focused(o))
            .map(|o| (o.mesh.clone(), o.matrix(), o.color))
            .collect();
        let view = View {
            // The orbit camera's yaw is measured from +X; azimuth 0 is its front (-90°).
            yaw: (azimuth - 90.0).to_radians(),
            pitch: elevation.to_radians(),
            width,
            height,
            background: Some(VIEWPORT_BG.to_array()),
            frame: Some(frame),
        };
        Ok(Some(Shot { parts, view }))
    }

    /// Draws the picture: RGBA rows, `width`×`height`. Can take a while on big scenes, so
    /// the app calls it off the UI thread.
    pub(super) fn render(&self) -> Vec<u8> {
        let meshes: Vec<Mesh> = self
            .parts
            .iter()
            .map(|(m, at, _)| m.transformed(at))
            .collect();
        let parts: Vec<(&Mesh, Rgba)> = meshes
            .iter()
            .zip(&self.parts)
            .map(|(m, (_, _, c))| (m, *c))
            .collect();
        thumbnail::render_view(&parts, &self.view)
    }

    pub(super) fn size(&self) -> [u32; 2] {
        [self.view.width, self.view.height]
    }
}

/// `view`, or `azimuth`/`elevation` with the iso view's for the one left out; iso when
/// neither is given.
fn angles(a: &Value) -> Result<(f32, f32), String> {
    let iso = (VIEWS[6].1, VIEWS[6].2);
    let free = !a["azimuth"].is_null() || !a["elevation"].is_null();
    match &a["view"] {
        Value::Null if !free => Ok(iso),
        Value::Null => {
            let degrees = |key: &str, default: f32| match &a[key] {
                Value::Null => Ok(default),
                v => v
                    .as_f64()
                    .map(|x| x as f32)
                    .filter(|x| x.is_finite())
                    .ok_or(format!("{key} must be a number of degrees")),
            };
            let elevation = degrees("elevation", iso.1)?;
            if !(-90.0..=90.0).contains(&elevation) {
                return Err("elevation must be between -90 and 90 degrees".into());
            }
            Ok((degrees("azimuth", iso.0)?, elevation))
        }
        _ if free => Err("give either view or azimuth/elevation, not both".into()),
        v => {
            let name = v.as_str().unwrap_or("");
            VIEWS
                .iter()
                .find(|(n, ..)| *n == name)
                .map(|(_, az, el)| (*az, *el))
                .ok_or_else(|| {
                    let names: Vec<&str> = VIEWS.iter().map(|(n, ..)| *n).collect();
                    format!("view must be one of {}", names.join(", "))
                })
        }
    }
}

/// The ids to frame; `None` for every object.
fn focus(scene: &Scene, v: &Value) -> Result<Option<BTreeSet<u64>>, String> {
    match v {
        Value::Null => Ok(None),
        Value::String(s) if s == "all" => Ok(None),
        Value::String(s) if s == "selection" => {
            if scene.selection.is_empty() {
                return Err("focus is selection, but nothing is selected".into());
            }
            Ok(Some(scene.selection.clone()))
        }
        Value::Array(items) => {
            let ids: BTreeSet<u64> = items
                .iter()
                .map(|v| v.as_u64().ok_or("focus ids must be integers"))
                .collect::<Result<_, _>>()?;
            if ids.is_empty() {
                return Err("focus needs at least one id".into());
            }
            let unknown: Vec<String> = ids
                .iter()
                .filter(|id| scene.get(**id).is_none())
                .map(u64::to_string)
                .collect();
            match unknown.len() {
                0 => Ok(Some(ids)),
                1 => Err(format!("no object with id {}", unknown[0])),
                _ => Err(format!("no objects with ids {}", unknown.join(", "))),
            }
        }
        _ => Err("focus must be \"all\", \"selection\" or a list of object ids".into()),
    }
}

/// `[width, height]` in pixels, clamped to [`SIDES`].
fn size(v: &Value) -> Result<[u32; 2], String> {
    if v.is_null() {
        return Ok(DEFAULT_SIZE);
    }
    let sides: Option<Vec<u64>> = v
        .as_array()
        .map(|a| a.iter().filter_map(Value::as_u64).collect());
    match (sides.as_deref(), v.as_array().map(Vec::len)) {
        (Some(&[w, h]), Some(2)) => {
            Ok([w, h].map(|s| (s.min(u32::MAX as u64) as u32).clamp(*SIDES.start(), *SIDES.end())))
        }
        _ => Err("size must be [width, height] in whole pixels".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A red cube at the origin and a green one 100 mm along +X; sizes 20 mm.
    fn scene() -> (Scene, u64, u64) {
        let mut scene = Scene::default();
        let mut cube = |color: Rgba, x: f32| {
            let mut o = Object::new("cube", Primitive::Cube.build(), color);
            o.scale = Vec3::splat(20.0 / o.mesh.aabb().size().x);
            o.translation = Vec3::new(x, 0.0, 10.0);
            scene.add(o)
        };
        let red = cube([255, 0, 0, 255], 0.0);
        let green = cube([0, 255, 0, 255], 100.0);
        (scene, red, green)
    }

    fn shot(scene: &Scene, a: Value) -> Result<Shot, String> {
        Shot::parse(scene, &a).map(|s| s.expect("an off-screen shot"))
    }

    /// Pixels where red or green is the strongest channel by far.
    fn count(px: &[u8], channel: usize) -> usize {
        px.chunks(4)
            .filter(|p| (0..3).all(|c| c == channel || p[channel] as u16 > p[c] as u16 + 60))
            .count()
    }

    /// Mean x (in pixels) of the pixels where `channel` dominates.
    fn mean_x(px: &[u8], width: usize, channel: usize) -> f32 {
        let xs: Vec<usize> = px
            .chunks(4)
            .enumerate()
            .filter(|(_, p)| (0..3).all(|c| c == channel || p[channel] as u16 > p[c] as u16 + 60))
            .map(|(i, _)| i % width)
            .collect();
        xs.iter().sum::<usize>() as f32 / xs.len().max(1) as f32
    }

    #[test]
    fn no_options_means_the_window_capture() {
        let (scene, ..) = scene();
        assert!(Shot::parse(&scene, &json!({})).unwrap().is_none());
        assert!(Shot::parse(&scene, &Value::Null).unwrap().is_none());
    }

    #[test]
    fn named_views_match_the_orbit_camera() {
        let (scene, ..) = scene();
        let dir = |a: Value| shot(&scene, a).unwrap().view.dir();
        let near = |a: Vec3, b: Vec3| (a - b).length() < 1e-3;
        // The app starts looking from -Y (front), so azimuth 0 is the default camera's.
        let cam = crate::camera::OrbitCamera::default();
        let start = (cam.eye() - cam.target).normalize();
        let flat = Vec3::new(start.x, start.y, 0.0).normalize();
        assert!(near(dir(json!({"view": "front"})), flat));
        assert!(near(dir(json!({"view": "front"})), -Vec3::Y));
        assert!(near(dir(json!({"view": "back"})), Vec3::Y));
        assert!(near(dir(json!({"view": "left"})), -Vec3::X));
        assert!(near(dir(json!({"view": "right"})), Vec3::X));
        assert!(near(dir(json!({"view": "top"})), Vec3::Z));
        assert!(near(dir(json!({"view": "bottom"})), -Vec3::Z));
        assert!(near(dir(json!({"azimuth": 90, "elevation": 0})), Vec3::X));
        let iso = dir(json!({"view": "iso"}));
        assert!(near(iso, Vec3::new(1.0, -1.0, 1.0).normalize()), "{iso}");
        // Top keeps X to the right and the front at the bottom of the picture.
        let top = shot(&scene, json!({"view": "top"})).unwrap().view;
        assert!(near(top.up(), Vec3::Y));
    }

    #[test]
    fn the_sides_appear_where_they_should() {
        let (scene, ..) = scene();
        // Red is at -X, green at +X: from the front red is on the left, from the back on
        // the right, and from the top too it is on the left.
        for (view, red_left) in [("front", true), ("back", false), ("top", true)] {
            let s = shot(&scene, json!({"view": view, "size": [200, 100]})).unwrap();
            let px = s.render();
            assert_eq!(px.len(), 200 * 100 * 4);
            let (r, g) = (mean_x(&px, 200, 0), mean_x(&px, 200, 1));
            assert_eq!(r < g, red_left, "{view}: red {r}, green {g}");
        }
        // From the left, red hides green.
        let px = shot(&scene, json!({"view": "left"})).unwrap().render();
        assert!(count(&px, 0) > 1000);
        assert!(count(&px, 1) < 20);
    }

    #[test]
    fn focus_frames_only_those_objects() {
        let (scene, red, _) = scene();
        let all = shot(&scene, json!({"view": "front"})).unwrap().render();
        let one = shot(&scene, json!({"view": "front", "focus": [red]}))
            .unwrap()
            .render();
        // Framed alone the red cube fills much more of the picture, and the green one,
        // 100 mm to the right, is out of it.
        assert!(count(&one, 0) > count(&all, 0) * 4);
        assert_eq!(count(&one, 1), 0);
        let centre = mean_x(&one, 800, 0);
        assert!((centre - 400.0).abs() < 4.0, "{centre}");
        // It is the picture's background around it, not the selection or the scene.
        assert!(scene.selection.is_empty());
    }

    #[test]
    fn isolate_hides_the_others() {
        let (scene, red, _) = scene();
        // From the right green stands in front of red and hides it, but not when isolated.
        let a = json!({"view": "right", "focus": [red], "isolate": false});
        let hidden = shot(&scene, a).unwrap().render();
        assert!(count(&hidden, 0) < 20);
        assert!(count(&hidden, 1) > 1000);
        let a = json!({"view": "right", "focus": [red], "isolate": true});
        let shown = shot(&scene, a).unwrap().render();
        assert!(count(&shown, 0) > 1000);
        assert_eq!(count(&shown, 1), 0);
    }

    #[test]
    fn focus_selection_uses_the_selection_without_changing_it() {
        let (mut scene, _, green) = scene();
        assert!(shot(&scene, json!({"focus": "selection"})).is_err());
        scene.selection.insert(green);
        let px = shot(&scene, json!({"view": "front", "focus": "selection"}))
            .unwrap()
            .render();
        assert!(count(&px, 1) > 1000);
        assert_eq!(scene.selection, BTreeSet::from([green]));
    }

    #[test]
    fn bad_arguments_are_errors() {
        let (scene, ..) = scene();
        let err = |a: Value| shot(&scene, a).err().expect("an error");
        assert!(err(json!({"view": "front", "azimuth": 10})).contains("not both"));
        assert!(err(json!({"view": "sideways"})).contains("one of front"));
        assert!(err(json!({"elevation": 120})).contains("between -90 and 90"));
        assert!(err(json!({"azimuth": "east"})).contains("number"));
        assert!(err(json!({"focus": [999]})).contains("999"));
        let both = err(json!({"focus": [998, 999]}));
        assert!(both.contains("998, 999"), "{both}");
        assert!(err(json!({"focus": []})).contains("at least one"));
        assert!(err(json!({"focus": "everything"})).contains("selection"));
        assert!(err(json!({"isolate": "yes"})).contains("isolate"));
        assert!(err(json!({"size": [100]})).contains("width, height"));
        assert!(err(json!({"size": [100, -5]})).contains("width, height"));
        let empty = Shot::parse(&Scene::default(), &json!({"view": "top"}));
        assert!(empty.is_err());
    }

    #[test]
    fn size_is_clamped_and_the_background_opaque() {
        let (scene, ..) = scene();
        let s = shot(&scene, json!({"size": [10, 5000]})).unwrap();
        assert_eq!(s.size(), [64, 2048]);
        assert_eq!(
            shot(&scene, json!({"isolate": false})).unwrap().size(),
            [800, 600]
        );
        let px = shot(&scene, json!({"size": [64, 64]})).unwrap().render();
        assert!(px.chunks(4).all(|p| p[3] == 255));
    }
}
