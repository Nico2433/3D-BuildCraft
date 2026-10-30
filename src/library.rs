//! 3D libraries: folders the user adds, browsed by subfolder with search.
//! Subfolders are categories; files the app can open are the items.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// How deep folders are searched; keeps a huge or looping tree from stalling the UI.
const MAX_DEPTH: usize = 8;
/// Most results a search returns.
pub const MAX_RESULTS: usize = 500;

fn is_model(p: &Path) -> bool {
    crate::io::Format::from_path(p).is_some()
}

/// Subfolders that hold models somewhere inside, then models, each sorted by name.
pub fn list(dir: &Path) -> Vec<Entry> {
    let Ok(read) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Entry> = read
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                return None;
            }
            let is_dir = path.is_dir();
            let keep = if is_dir {
                has_models(&path, MAX_DEPTH)
            } else {
                is_model(&path)
            };
            keep.then_some(Entry { path, name, is_dir })
        })
        .collect();
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn has_models(dir: &Path, depth: usize) -> bool {
    depth > 0
        && std::fs::read_dir(dir).is_ok_and(|r| {
            r.flatten().any(|e| {
                let p = e.path();
                if p.is_dir() {
                    has_models(&p, depth - 1)
                } else {
                    is_model(&p)
                }
            })
        })
}

/// Models under `dir` whose file name contains `query` (case-insensitive), by name.
pub fn search(dir: &Path, query: &str) -> Vec<Entry> {
    let q = query.to_lowercase();
    let mut out = Vec::new();
    let mut stack = vec![(dir.to_path_buf(), 0)];
    while let Some((d, depth)) = stack.pop() {
        let Ok(read) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in read.flatten() {
            let path = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if path.is_dir() {
                if depth + 1 < MAX_DEPTH {
                    stack.push((path, depth + 1));
                }
            } else if is_model(&path) && name.to_lowercase().contains(&q) {
                out.push(Entry {
                    path,
                    name,
                    is_dir: false,
                });
                if out.len() >= MAX_RESULTS {
                    out.sort_by_key(|e| e.name.to_lowercase());
                    return out;
                }
            }
        }
    }
    out.sort_by_key(|e| e.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_folders_with_models_then_models() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("Kitchen/Cups")).unwrap();
        std::fs::create_dir_all(root.join("Empty")).unwrap();
        std::fs::write(root.join("Kitchen/Cups/mug.stl"), b"").unwrap();
        std::fs::write(root.join("b.3mf"), b"").unwrap();
        std::fs::write(root.join("A.obj"), b"").unwrap();
        std::fs::write(root.join("notes.txt"), b"").unwrap();
        let names: Vec<String> = list(root).into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["Kitchen", "A.obj", "b.3mf"]);
        let found: Vec<String> = search(root, "MUG").into_iter().map(|e| e.name).collect();
        assert_eq!(found, ["mug.stl"]);
        assert!(list(&root.join("missing")).is_empty());
    }
}
