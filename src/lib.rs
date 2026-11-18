//! 3D BuildCraft core: meshes, file formats and scene model, usable without the UI.

pub mod assistant;
pub mod boolean;
pub mod camera;
pub mod cut;
pub mod emboss;
pub mod history;
pub mod host;
pub mod i18n;
pub mod image3d;
pub mod io;
pub mod library;
pub mod mcp;
pub mod measure;
pub mod mesh;
pub mod mesh_cache;
pub mod outline;
pub mod paper;
pub mod picture;
pub mod primitives;
pub mod repair;
pub mod scene;
pub mod section;
pub mod settings;
pub mod slicer;
pub mod snapping;
pub mod solid;
pub mod texture;
pub mod texture_gen;
pub mod thumbnail;
pub mod update;
pub mod webcam;

/// Runs `f`, turning a panic inside it into an error, so a bug in a geometry library
/// fails that one operation instead of closing the app. `f` must leave nothing half
/// changed when it panics: compute first, then change the scene.
pub fn no_panic<T>(f: impl FnOnce() -> anyhow::Result<T>) -> anyhow::Result<T> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|payload| {
        let why = payload
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown error".into());
        Err(anyhow::anyhow!(
            "The operation failed inside the geometry code ({why})"
        ))
    })
}
