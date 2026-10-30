// No console window on Windows; agents still get stdin/stdout for --mcp, as they pipe them.
#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod render;

// Re-exported at the binary's root so app.rs and render.rs can write `crate::mesh`, etc.
use buildcraft3d::*;

use eframe::egui;
use std::path::PathBuf;

const APP_ID: &str = "io.github.nico2433.BuildCraft3D";

const USAGE: &str = "usage: buildcraft3d [model] [--screenshot out.png] [--size WxH] \
[--tab insert|object|edit|paint|view|help] [--lang es|en] [--select-all] [--menu] \
[--zoom F] [--image file] [--library dir] [--mcp] [--mode move|rotate|scale] [--tool settle|cut|simplify|hollow|emboss|measure|paint|paint-colors|paint-more|custom|camera|ai|chat|unsaved|repairing|grid|recover|start|bar-edit|add|print|print-material|print-design|print-about]";

fn main() -> eframe::Result<()> {
    env_logger::init();
    let mut launch = app::Launch::default();
    let mut size = [1280.0, 800.0];
    let mut args = std::env::args_os().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || args.next().and_then(|v| v.into_string().ok());
        match arg.to_str() {
            Some("--screenshot") => launch.screenshot = value().map(PathBuf::from),
            Some("--tab") => launch.tab = value(),
            Some("--lang") => {
                launch.lang = value().map(|l| i18n::Lang::from_locale(&l));
            }
            Some("--size") => {
                let v = value().unwrap_or_default();
                match v.split_once('x').map(|(w, h)| (w.parse(), h.parse())) {
                    Some((Ok(w), Ok(h))) => size = [w, h],
                    _ => eprintln!("{USAGE}"),
                }
            }
            Some("--select-all") => launch.select_all = true,
            Some("--menu") => launch.menu = true,
            Some("--zoom") => launch.zoom = value().and_then(|v| v.parse().ok()),
            Some("--tool") => launch.tool = value(),
            Some("--image") => launch.image = value().map(PathBuf::from),
            Some("--library") => launch.library = value().map(PathBuf::from),
            Some("--mcp") => launch.mcp = true,
            Some("--mode") => launch.mode = value(),
            Some(s) if s.starts_with("--") => {
                eprintln!("{USAGE}");
                std::process::exit(2);
            }
            _ => launch.file = Some(PathBuf::from(arg)),
        }
    }
    // With the app already open, an agent's `--mcp` works on that window.
    if launch.mcp {
        if let Some(done) = mcp::bridge() {
            if let Err(e) = done {
                eprintln!("{e:#}");
                std::process::exit(1);
            }
            return Ok(());
        }
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        multisampling: render::MSAA_SAMPLES as u16,
        depth_buffer: 24,
        viewport: egui::ViewportBuilder::default()
            .with_title("3D BuildCraft")
            .with_app_id(APP_ID)
            .with_icon(egui::IconData {
                rgba: image::load_from_memory(include_bytes!("../packaging/icon.png"))
                    .expect("embedded window icon")
                    .into_rgba8()
                    .into_raw(),
                width: 256,
                height: 256,
            })
            .with_inner_size(size)
            .with_min_inner_size([800.0, 500.0])
            // The app draws its own title bar, like the original.
            .with_decorations(false)
            .with_drag_and_drop(true),
        event_loop_builder: Some(Box::new(|builder| {
            // winit takes no dropped files on Wayland, so run through XWayland when it is
            // there; BUILDCRAFT3D_WAYLAND=1 keeps native Wayland (no drag and drop).
            #[cfg(target_os = "linux")]
            if std::env::var_os("DISPLAY").is_some()
                && std::env::var_os("BUILDCRAFT3D_WAYLAND").is_none()
            {
                winit::platform::x11::EventLoopBuilderExtX11::with_x11(builder);
            }
            #[cfg(not(target_os = "linux"))]
            let _ = builder;
        })),
        ..Default::default()
    };
    eframe::run_native(
        "3D BuildCraft",
        options,
        Box::new(move |cc| Ok(Box::new(app::App::new(cc, launch)))),
    )
}
