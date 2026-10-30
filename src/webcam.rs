//! Webcam: frames from the first camera, read on a background thread so the UI never
//! waits on the device. The camera is made inside the thread because on some systems it
//! cannot move between threads.

use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// The latest frame, a counter that grows with each new one, or why there is none.
type Latest = Result<(u64, Arc<RgbaImage>), String>;

pub struct Webcam {
    latest: Arc<Mutex<Option<Latest>>>,
    stop: Arc<AtomicBool>,
}

impl Webcam {
    /// Starts the first camera. Frames (or the error) arrive later through [`Webcam::latest`].
    pub fn open() -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let (out, quit) = (latest.clone(), stop.clone());
        std::thread::spawn(move || {
            if let Err(e) = run(&out, &quit) {
                log::warn!("camera: {e}");
                *out.lock().unwrap() = Some(Err(e.to_string()));
            }
        });
        Self { latest, stop }
    }

    /// `None` while the camera is starting.
    pub fn latest(&self) -> Option<Latest> {
        self.latest.lock().unwrap().clone()
    }
}

impl Drop for Webcam {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

fn run(out: &Mutex<Option<Latest>>, stop: &AtomicBool) -> Result<(), nokhwa::NokhwaError> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    // macOS asks the user for camera access; elsewhere this does nothing.
    nokhwa::nokhwa_initialize(|_| {});
    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut cam = nokhwa::Camera::new(CameraIndex::Index(0), format)?;
    cam.open_stream()?;
    let mut n = 0;
    while !stop.load(Ordering::Relaxed) {
        let rgb = cam.frame()?.decode_image::<RgbFormat>()?;
        n += 1;
        let rgba = image::DynamicImage::ImageRgb8(rgb).into_rgba8();
        *out.lock().unwrap() = Some(Ok((n, Arc::new(rgba))));
    }
    cam.stop_stream()
}
