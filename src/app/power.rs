//! Suspend drawing and release disposable resources while the viewport is minimized.
use super::*;

#[derive(Default)]
pub(super) struct State {
    minimized: bool,
}

#[derive(Debug, PartialEq, Eq)]
enum Transition {
    Enter,
    Leave,
    None,
}

impl State {
    fn transition(&mut self, minimized: Option<bool>) -> Transition {
        let minimized = minimized.unwrap_or(self.minimized);
        let previous = std::mem::replace(&mut self.minimized, minimized);
        match (previous, minimized) {
            (false, true) => Transition::Enter,
            (true, false) => Transition::Leave,
            _ => Transition::None,
        }
    }

    pub fn sleeping(&self) -> bool {
        self.minimized
    }
}

impl App {
    /// Returns true when only background work should be serviced. egui 0.31 has
    /// no viewport visibility/occlusion flag; lack of focus is not minimization.
    pub(super) fn power_update(&mut self, ctx: &egui::Context, frame: &eframe::Frame) -> bool {
        match self.power.transition(ctx.input(|i| i.viewport().minimized)) {
            Transition::Enter => {
                log_memory("before release");
                log::info!(
                    "power: retaining {} scene objects and undo history",
                    self.scene.objects.len()
                );
                self.picking.release();
                crate::scene::clear_caches();
                // Replacing the channels also discards decoded results and queued
                // work. An active thumbnail finishes, then its disconnected worker exits.
                self.thumbs.release();
                ctx.forget_all_images();
                ctx.clear_animations();
                if let Some(Tool::Camera { cam, tex }) = &mut self.tool {
                    cam.set_paused(true);
                    *tex = None;
                }
                if let Some(Tool::Image { preview, built, .. }) = &mut self.tool {
                    *preview = Err(String::new());
                    *built = None;
                }
                if let Some(rs) = frame.wgpu_render_state() {
                    if let Some(renderer) =
                        rs.renderer.write().callback_resources.get_mut::<Renderer>()
                    {
                        renderer.release_meshes();
                    }
                }
                // Finish the camera movement without scheduling animation frames.
                if let Some((_, to, _)) = self.glide.take() {
                    self.camera = to;
                }
                self.last_frame = None;
                trim_heap();
                log_memory("after release");
            }
            Transition::Leave => {
                if let Some(Tool::Camera { cam, .. }) = &self.tool {
                    cam.set_paused(false);
                }
                self.last_frame = None;
            }
            Transition::None => {}
        }
        if !self.power.sleeping() {
            return false;
        }
        // Workers and MCP callers wake the event loop only when they have results.
        // No UI is built here, so progress, FPS and animation cannot schedule frames.
        let jobs_before = self.jobs.len();
        self.collect_jobs();
        let actions_pending = !self.actions.is_empty();
        for action in std::mem::take(&mut self.actions) {
            self.apply(action, ctx);
        }
        let processed = self.serve_agent(ctx);
        self.sync_listener(ctx);
        self.chat_events();
        if processed || actions_pending || self.jobs.len() != jobs_before {
            crate::scene::clear_caches();
            trim_heap();
        }
        if self.autosave.due(self.scene.dirty) {
            self.autosave.start(self.scene.objects.clone());
        }
        true
    }
}

fn trim_heap() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    // SAFETY: glibc's thread-safe allocator hint has no pointer arguments. libc
    // was already present in Cargo.lock; no additional package is introduced.
    unsafe {
        libc::malloc_trim(0);
    }
}

fn log_memory(stage: &str) {
    #[cfg(target_os = "linux")]
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        let memory: Vec<_> = status
            .lines()
            .filter(|l| l.starts_with("VmRSS:") || l.starts_with("RssAnon:"))
            .collect();
        log::info!("power: {stage}: {}", memory.join(", "));
    }
    #[cfg(not(target_os = "linux"))]
    log::info!("power: {stage}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_once_and_resumes() {
        let mut state = State::default();
        assert_eq!(state.transition(None), Transition::None);
        assert_eq!(state.transition(Some(false)), Transition::None);
        assert_eq!(state.transition(Some(true)), Transition::Enter);
        assert!(state.sleeping());
        assert_eq!(state.transition(Some(true)), Transition::None);
        assert_eq!(state.transition(None), Transition::None);
        assert!(state.sleeping());
        assert_eq!(state.transition(Some(false)), Transition::Leave);
        assert!(!state.sleeping());
        assert_eq!(state.transition(Some(false)), Transition::None);
        assert_eq!(state.transition(Some(true)), Transition::Enter);
    }
}
