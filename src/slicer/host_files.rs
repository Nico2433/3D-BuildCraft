//! Reading files our Flatpak sandbox hides (another app's `~/.var/app`) through the host.

use crate::host::{host_command, in_flatpak};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// Files read through the host, by path, with when they were read: a folder is read in one
/// host call, since one per preset takes seconds for a vendor's hundreds of profiles. The
/// folder itself is listed too, with no text, so a file missing from it is not asked for.
static HOST_FILES: std::sync::Mutex<Vec<(PathBuf, std::time::Instant, String)>> =
    std::sync::Mutex::new(Vec::new());

/// How long a file read through the host stays current: one lookup, not across them, so
/// changes made in the slicer show on the next one.
const HOST_FILES_FRESH: std::time::Duration = std::time::Duration::from_secs(5);

/// The names of the `.json` files in a folder, through the host when our sandbox hides it.
pub(super) fn list_json(dir: &Path) -> Vec<String> {
    let names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok()?.file_name().into_string().ok())
            .collect(),
        Err(_) if in_flatpak() => read_host_folder(dir),
        Err(_) => Vec::new(),
    };
    let mut names: Vec<String> = names
        .iter()
        .filter_map(|n| n.strip_suffix(".json").map(str::to_owned))
        .collect();
    names.sort();
    names
}

/// Reads every `.json` file of a hidden folder through the host into [`HOST_FILES`] and
/// returns their names.
fn read_host_folder(dir: &Path) -> Vec<String> {
    // Name, NUL, contents, NUL for each file; JSON holds no NUL.
    let script = r#"cd "$1" && for f in *.json; do [ -f "$f" ] && printf '%s\0' "$f" && cat -- "$f" && printf '\0'; done"#;
    let Ok(out) = host_command("sh")
        .args(["-c", script, "sh"])
        .arg(dir)
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    let parts: Vec<&str> = text.split('\0').collect();
    let now = std::time::Instant::now();
    let mut files = HOST_FILES.lock().unwrap_or_else(|e| e.into_inner());
    files.retain(|(path, at, _)| path.parent() != Some(dir) && at.elapsed() < HOST_FILES_FRESH);
    files.push((dir.to_path_buf(), now, String::new()));
    let mut names = Vec::new();
    for pair in parts.chunks_exact(2) {
        files.push((dir.join(pair[0]), now, pair[1].to_owned()));
        names.push(pair[0].to_owned());
    }
    names
}

/// Reads a JSON file, through the host when our sandbox hides it (another app's `~/.var/app`).
pub(super) fn read_json(path: &Path) -> Option<Value> {
    let host_file = |path: &Path| {
        let files = HOST_FILES.lock().unwrap_or_else(|e| e.into_inner());
        files
            .iter()
            .find(|(p, at, _)| p == path && at.elapsed() < HOST_FILES_FRESH)
            .map(|(_, _, text)| text.clone())
    };
    let text = std::fs::read_to_string(path).ok().or_else(|| {
        if !in_flatpak() {
            return None;
        }
        // A preset's folder holds its siblings, which the same lookup is about to read.
        if path.extension().is_some_and(|e| e == "json") {
            let dir = path.parent()?;
            if host_file(dir).is_none() {
                read_host_folder(dir);
            }
            return host_file(path);
        }
        host_file(path).or_else(|| {
            let out = host_command("cat").arg(path).output().ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
        })
    })?;
    serde_json::from_str(&text).ok()
}
