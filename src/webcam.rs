//! Webcam: frames from the first camera, read on a background thread so the UI never
//! waits on the device. The camera is made inside the thread because on some systems it
//! cannot move between threads.

use image::RgbaImage;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// Whether the system lists a camera, so the UI can disable what needs one.
pub fn available() -> bool {
    nokhwa::query(nokhwa::utils::ApiBackend::Auto).is_ok_and(|cameras| !cameras.is_empty())
}

/// The latest frame, a counter that grows with each new one, or why there is none.
type Latest = Result<(u64, Arc<RgbaImage>), String>;

pub struct Webcam {
    latest: Arc<Mutex<Option<Latest>>>,
    stop: Arc<AtomicBool>,
    paused: Arc<(Mutex<bool>, Condvar)>,
}

impl Webcam {
    /// Starts the first camera. Frames (or the error) arrive later through [`Webcam::latest`].
    pub fn open() -> Self {
        let latest = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let paused = Arc::new((Mutex::new(false), Condvar::new()));
        let pause = paused.clone();
        let (out, quit) = (latest.clone(), stop.clone());
        std::thread::spawn(move || {
            if let Err(e) = run(&out, &quit, &pause) {
                log::warn!("camera: {e}");
                *out.lock().unwrap() = Some(Err(e.to_string()));
            }
        });
        Self {
            latest,
            stop,
            paused,
        }
    }

    /// Suspends capture and discards the decoded preview until the window returns.
    pub fn set_paused(&self, paused: bool) {
        *self.paused.0.lock().unwrap() = paused;
        if paused {
            *self.latest.lock().unwrap() = None;
        }
        self.paused.1.notify_all();
    }

    /// `None` while the camera is starting.
    pub fn latest(&self) -> Option<Latest> {
        self.latest.lock().unwrap().clone()
    }
}

impl Drop for Webcam {
    fn drop(&mut self) {
        let _pause = self.paused.0.lock().unwrap();
        self.stop.store(true, Ordering::Relaxed);
        self.paused.1.notify_all();
    }
}

fn run(
    out: &Mutex<Option<Latest>>,
    stop: &AtomicBool,
    paused: &(Mutex<bool>, Condvar),
) -> Result<(), nokhwa::NokhwaError> {
    use nokhwa::pixel_format::RgbFormat;
    use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
    // macOS asks the user for camera access; elsewhere this does nothing.
    nokhwa::nokhwa_initialize(|_| {});
    let format = RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestResolution);
    let mut cam = nokhwa::Camera::new(CameraIndex::Index(0), format)?;
    cam.open_stream()?;
    let mut n = 0;
    while !stop.load(Ordering::Relaxed) {
        let mut pause = paused.0.lock().unwrap();
        if *pause {
            cam.stop_stream()?;
            while *pause && !stop.load(Ordering::Relaxed) {
                pause = paused.1.wait(pause).unwrap();
            }
            if stop.load(Ordering::Relaxed) {
                break;
            }
            cam.open_stream()?;
        }
        drop(pause);
        let rgb = cam.frame()?.decode_image::<RgbFormat>()?;
        n += 1;
        let rgba = image::DynamicImage::ImageRgb8(rgb).into_rgba8();
        let pause = paused.0.lock().unwrap();
        if !*pause {
            *out.lock().unwrap() = Some(Ok((n, Arc::new(rgba))));
        }
    }
    cam.stop_stream()
}
