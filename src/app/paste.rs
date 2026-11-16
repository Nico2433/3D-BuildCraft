//! Ctrl+V and Paste: copied objects, or a picture or files from the system clipboard.
//! A picture opens Load image as a dropped one does; files are imported as a drop.

use super::*;
use std::sync::mpsc;

/// How often, at most, the ribbon asks the system clipboard whether it holds something
/// to paste.
const PROBE_INTERVAL: Duration = Duration::from_secs(1);

/// What a paste does.
#[derive(Debug, PartialEq)]
pub(super) enum PasteChoice {
    /// Pastes the objects copied in the app.
    Objects,
    /// Opens Load image with this picture.
    Image(PathBuf),
    /// Imports these files as if they were dropped.
    Files(Vec<PathBuf>),
    Nothing,
}

/// Chooses what a paste does: objects copied here (our marker text on the system
/// clipboard), else a picture, else copied files, else any objects copied earlier.
/// `image` and `files` read the clipboard and are only called when needed.
pub(super) fn paste_choice(
    text: Option<&str>,
    have_objects: bool,
    image: impl FnOnce() -> Option<PathBuf>,
    files: impl FnOnce() -> Vec<PathBuf>,
) -> PasteChoice {
    if text == Some(CLIP_TEXT) {
        return PasteChoice::Objects;
    }
    if let Some(p) = image() {
        return PasteChoice::Image(p);
    }
    let files = files();
    if !files.is_empty() {
        PasteChoice::Files(files)
    } else if have_objects {
        PasteChoice::Objects
    } else {
        PasteChoice::Nothing
    }
}

/// The files a clipboard text names that the app opens (models and pictures): a
/// `text/uri-list` of `file://` URIs, GNOME's `copy`/`cut` list or plain paths, one per
/// line. Comments, other URIs and other files are left out.
pub(super) fn parse_file_list(text: &str) -> Vec<PathBuf> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .filter_map(|l| match l.strip_prefix("file://") {
            Some(rest) => file_uri_path(rest),
            None => Path::new(l).is_absolute().then(|| PathBuf::from(l)),
        })
        .filter(|p| supported(p))
        .collect()
}

/// The path of a `file://` URI, given what follows `file://`.
fn file_uri_path(rest: &str) -> Option<PathBuf> {
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    if !rest.starts_with('/') {
        return None;
    }
    let path = percent_decode(rest)?;
    // `file:///C:/x` is `C:/x` on Windows.
    let path = match path.as_bytes() {
        [b'/', drive, b':', ..] if cfg!(windows) && drive.is_ascii_alphabetic() => &path[1..],
        _ => &path,
    };
    Some(PathBuf::from(path))
}

/// `%20` and the like decoded; None when the result is not UTF-8 or an escape is broken.
fn percent_decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' {
            let hex = std::str::from_utf8(b.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// A model the app imports or a picture Load image opens, by its extension.
fn supported(path: &Path) -> bool {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    import::is_image(path) || io::OPEN_EXTENSIONS.contains(&ext.as_str())
}

/// Ctrl+V as egui-winit reports it. With text on the clipboard it sends a Paste event;
/// with none (a picture, say) it sends nothing for the press, as it takes it for a
/// paste, and only the V key's release arrives. A release with no press before it is
/// therefore a Ctrl+V.
#[derive(Default)]
pub(super) struct PasteKey {
    /// V's press was seen, so its release is ordinary typing.
    v_down: bool,
    /// A Paste event already answered this press.
    pasted: bool,
}

impl PasteKey {
    /// Follows one input event; Some for a paste, with the clipboard's text when known.
    pub fn on_event(&mut self, e: &egui::Event) -> Option<Option<String>> {
        match e {
            egui::Event::Paste(text) => {
                self.pasted = true;
                Some(Some(text.clone()))
            }
            egui::Event::Key {
                key: Key::V,
                pressed,
                ..
            } => {
                if *pressed {
                    self.v_down = true;
                    return None;
                }
                let swallowed = !self.v_down && !self.pasted;
                *self = Self::default();
                swallowed.then_some(None)
            }
            _ => None,
        }
    }
}

/// Paste state: Ctrl+V tracking and whether the system clipboard has something to paste,
/// asked on a thread at most every `PROBE_INTERVAL`.
#[derive(Default)]
pub(super) struct PasteState {
    pub key: PasteKey,
    /// The last answer and when it was asked.
    available: Option<(Instant, bool)>,
    probe: Option<mpsc::Receiver<bool>>,
}

impl PasteState {
    /// True when the system clipboard last held a picture or files to paste. Asks again
    /// on a thread when the answer is old; the new one shows on a later frame.
    pub fn clipboard_has_something(&mut self) -> bool {
        if let Some(rx) = &self.probe {
            match rx.try_recv() {
                Ok(v) => {
                    self.available = Some((Instant::now(), v));
                    self.probe = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.probe = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let stale = self
            .available
            .is_none_or(|(at, _)| at.elapsed() >= PROBE_INTERVAL);
        if stale && self.probe.is_none() {
            let (tx, rx) = mpsc::channel();
            self.probe = Some(rx);
            std::thread::spawn(move || {
                let _ = tx.send(clipboard_has_something());
            });
        }
        self.available.is_some_and(|(_, v)| v)
    }
}

/// Asks the system clipboard for a picture or files to paste (not our objects' marker).
fn clipboard_has_something() -> bool {
    let Ok(mut c) = arboard::Clipboard::new() else {
        return false;
    };
    let text = c.get_text().ok();
    if text.as_deref() == Some(CLIP_TEXT) {
        return false;
    }
    !clipboard_files(&mut c, text.as_deref()).is_empty() || c.get_image().is_ok()
}

/// Copied files the app opens: the clipboard's file list, else paths in its text.
fn clipboard_files(c: &mut arboard::Clipboard, text: Option<&str>) -> Vec<PathBuf> {
    let listed: Vec<PathBuf> = c
        .get()
        .file_list()
        .unwrap_or_default()
        .into_iter()
        .filter(|p| supported(p))
        .collect();
    let files = if listed.is_empty() {
        text.map(parse_file_list).unwrap_or_default()
    } else {
        listed
    };
    files.into_iter().filter(|p| p.is_file()).collect()
}

/// Saves the clipboard's picture as a PNG in the cache folder, for Load image. Pictures
/// pasted earlier are removed, all but this one.
fn clipboard_image(c: &mut arboard::Clipboard) -> Option<PathBuf> {
    let img = c.get_image().ok()?;
    let rgba =
        image::RgbaImage::from_raw(img.width as u32, img.height as u32, img.bytes.into_owned())?;
    let dir = crate::settings::dirs()
        .map(|d| d.cache_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir)
        .join("pasted");
    std::fs::create_dir_all(&dir).ok()?;
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or_default();
    let path = dir.join(format!("pasted-{stamp}.png"));
    if let Err(e) = rgba.save(&path) {
        log::error!("could not save the pasted picture: {e}");
        return None;
    }
    for old in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        if old.path() != path {
            let _ = std::fs::remove_file(old.path());
        }
    }
    Some(path)
}

impl App {
    /// Ctrl+V or Paste. `text` is the clipboard's text when the window already read it.
    pub(super) fn paste(&mut self, text: Option<String>, ctx: &egui::Context) {
        let mut sys = arboard::Clipboard::new().ok();
        let text = text.or_else(|| sys.as_mut()?.get_text().ok());
        // Each reader takes the clipboard in turn; only the first that is needed runs.
        let sys = std::cell::RefCell::new(sys);
        let choice = paste_choice(
            text.as_deref(),
            !self.clipboard.is_empty(),
            || clipboard_image(sys.borrow_mut().as_mut()?),
            || {
                sys.borrow_mut()
                    .as_mut()
                    .map(|c| clipboard_files(c, text.as_deref()))
                    .unwrap_or_default()
            },
        );
        match choice {
            PasteChoice::Objects => {
                let clip = self.clipboard.clone();
                self.edit(|s| s.paste(&clip));
            }
            PasteChoice::Image(p) => self.tool = Some(Tool::image(p)),
            PasteChoice::Files(paths) => {
                let at = ctx
                    .input(|i| i.pointer.hover_pos())
                    .and_then(|p| self.floor_point(p))
                    .unwrap_or(Vec3::ZERO);
                self.open_dropped(paths, at, ctx);
            }
            PasteChoice::Nothing => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(pressed: bool) -> egui::Event {
        egui::Event::Key {
            key: Key::V,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    }

    #[test]
    fn a_uri_list_or_paths_give_the_supported_files() {
        // A plain absolute path, as the platform writes one.
        let plain = if cfg!(windows) {
            r"C:\a\plain.3mf"
        } else {
            "/home/a/plain.3mf"
        };
        let text = format!(
            "# from a file manager\r\nfile:///home/a/My%20Part.STL\n\
             file://localhost/home/a/photo.jpg\nfile:///home/a/notes.txt\n\
             https://example.com/x.stl\n{plain}\nrelative.obj\n\n"
        );
        assert_eq!(
            parse_file_list(&text),
            ["/home/a/My Part.STL", "/home/a/photo.jpg", plain].map(PathBuf::from)
        );
        // GNOME's list starts with the operation.
        assert_eq!(
            parse_file_list("copy\nfile:///tmp/a.ply"),
            [PathBuf::from("/tmp/a.ply")]
        );
        assert!(parse_file_list("file:///tmp/bad%2.stl").is_empty());
        assert!(parse_file_list("just some text").is_empty());
    }

    #[test]
    fn paste_prefers_our_objects_then_a_picture_then_files() {
        let png = || Some(PathBuf::from("/c/p.png"));
        let none = || None;
        let files = || vec![PathBuf::from("/m/a.stl")];
        let no_files = Vec::new;
        // Our marker: the objects, without reading the clipboard further.
        let choice = paste_choice(
            Some(CLIP_TEXT),
            true,
            || panic!("no image read"),
            || panic!("no files read"),
        );
        assert_eq!(choice, PasteChoice::Objects);
        assert_eq!(
            paste_choice(Some("x"), true, png, files),
            PasteChoice::Image(PathBuf::from("/c/p.png"))
        );
        assert_eq!(
            paste_choice(None, true, none, files),
            PasteChoice::Files(files())
        );
        assert_eq!(
            paste_choice(Some("hello"), true, none, no_files),
            PasteChoice::Objects
        );
        assert_eq!(
            paste_choice(None, false, none, no_files),
            PasteChoice::Nothing
        );
    }

    #[test]
    fn ctrl_v_without_text_is_told_by_a_lone_release_of_v() {
        let mut k = PasteKey::default();
        // Typing v: press and release.
        assert_eq!(k.on_event(&key(true)), None);
        assert_eq!(k.on_event(&key(false)), None);
        // Ctrl+V with text: the Paste event; the release adds nothing.
        let paste = egui::Event::Paste("t".into());
        assert_eq!(k.on_event(&paste), Some(Some("t".into())));
        assert_eq!(k.on_event(&key(false)), None);
        // Ctrl+V with a picture: the press was swallowed, only the release comes.
        assert_eq!(k.on_event(&key(false)), Some(None));
        assert_eq!(k.on_event(&key(true)), None);
        assert_eq!(k.on_event(&key(false)), None);
    }
}
