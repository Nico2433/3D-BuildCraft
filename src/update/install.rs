//! Putting a downloaded update in place and starting the new version. Nothing downloaded
//! runs except the one installer step of each kind (the Flatpak bundle through `flatpak
//! install`, the MSI through `msiexec`, or the new AppImage or exe when it restarts).

use super::{asset_for, fetch, PackageKind, Release};
use anyhow::{bail, Context, Result};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::AtomicU32;

/// The exe the portable zip carries.
const PORTABLE_EXE: &str = "buildcraft3d.exe";

/// What starts the new version once the update is in place.
#[derive(Debug, PartialEq)]
pub enum Restart {
    /// `flatpak run` on the host.
    Flatpak { app_id: String },
    /// The replaced AppImage or portable exe.
    Exe(PathBuf),
    /// The MSI, run by `msiexec` after the app closes; it does not reopen the app.
    Msi(PathBuf),
}

/// Downloads the release's package for `kind`, checks it and installs it, short of
/// restarting. `progress` counts thousandths of the download.
pub fn prepare(kind: &PackageKind, release: &Release, progress: &AtomicU32) -> Result<Restart> {
    let Some(asset) = asset_for(release, kind) else {
        bail!("the release has no package for this installation");
    };
    let download = |dest: &Path| -> Result<()> {
        fetch::download(asset, dest, progress)?;
        if let Err(e) = fetch::verify(release, asset, dest) {
            let _ = std::fs::remove_file(dest);
            return Err(e);
        }
        Ok(())
    };
    match kind {
        PackageKind::Flatpak { system } => {
            let bundle = staging_dir()?.join(&asset.name);
            download(&bundle)?;
            let status = crate::host::host_command("flatpak")
                .args(flatpak_install_args(&bundle, *system))
                .status()
                .context("could not run flatpak on the host")?;
            let _ = std::fs::remove_file(&bundle);
            if !status.success() {
                bail!("flatpak install failed ({status})");
            }
            Ok(Restart::Flatpak {
                app_id: super::flatpak_app_id(),
            })
        }
        PackageKind::AppImage(path) => {
            let temp = sibling(path, ".update")?;
            download(&temp)?;
            make_executable(&temp)?;
            std::fs::rename(&temp, path)
                .with_context(|| format!("could not replace {}", path.display()))?;
            Ok(Restart::Exe(path.clone()))
        }
        PackageKind::Msi => {
            let msi = staging_dir()?.join(&asset.name);
            download(&msi)?;
            Ok(Restart::Msi(msi))
        }
        PackageKind::Portable(exe) => {
            let zip = staging_dir()?.join(&asset.name);
            download(&zip)?;
            let new = sibling(exe, ".new")?;
            extract_exe(&zip, &new)?;
            let _ = std::fs::remove_file(&zip);
            swap_running_exe(exe, &new)?;
            Ok(Restart::Exe(exe.clone()))
        }
        PackageKind::Other => bail!("this installation cannot update itself"),
    }
}

/// Starts the new version (or, for the MSI, its installer); the caller then closes.
pub fn restart(r: &Restart) -> Result<()> {
    let mut command = match r {
        Restart::Flatpak { app_id } => {
            let mut c = crate::host::host_command("flatpak");
            c.args(["run", app_id]);
            c
        }
        Restart::Exe(path) => {
            let mut c = Command::new(path);
            // The AppImage runtime sets these again for the new image.
            for var in ["APPDIR", "APPIMAGE", "ARGV0", "OWD"] {
                c.env_remove(var);
            }
            if let Some(dir) = path.parent() {
                c.current_dir(dir);
            }
            c
        }
        Restart::Msi(msi) => {
            let mut c = Command::new("msiexec");
            c.args(msi_args(msi));
            c
        }
    };
    let mut child = command.spawn().context("could not start the new version")?;
    // Reaped if this process outlives it.
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// Removes the exe a portable update set aside last time (Windows cannot delete a running
/// exe, only rename it).
pub fn remove_previous() {
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(old) = sibling(&exe, ".old") {
            if old.exists() {
                let _ = std::fs::remove_file(old);
            }
        }
    }
}

/// `flatpak` arguments that install the bundle over the current version.
pub fn flatpak_install_args(bundle: &Path, system: bool) -> Vec<OsString> {
    let scope = if system { "--system" } else { "--user" };
    ["install", scope, "-y", "--reinstall", "--bundle"]
        .into_iter()
        .map(OsString::from)
        .chain([bundle.as_os_str().to_owned()])
        .collect()
}

/// `msiexec` arguments: install (an upgrade, as the version is higher) with a progress bar
/// and no questions.
pub fn msi_args(msi: &Path) -> Vec<OsString> {
    vec!["/i".into(), msi.as_os_str().to_owned(), "/passive".into()]
}

/// A folder for downloads that the host can read too: the app's cache, which inside the
/// Flatpak is `~/.var/app/<id>/cache` at the same path on the host.
fn staging_dir() -> Result<PathBuf> {
    let dir = crate::settings::dirs()
        .map(|d| d.cache_dir().join("update"))
        .unwrap_or_else(|| std::env::temp_dir().join("buildcraft3d-update"));
    std::fs::create_dir_all(&dir).with_context(|| format!("could not create {}", dir.display()))?;
    Ok(dir)
}

/// `path` with `suffix` added to its file name, in the same folder (so a rename is atomic).
fn sibling(path: &Path, suffix: &str) -> Result<PathBuf> {
    let mut name = path.file_name().context("no file name")?.to_owned();
    name.push(suffix);
    Ok(path.with_file_name(name))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))?;
    Ok(())
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<()> {
    Ok(())
}

/// Writes the zip's `buildcraft3d.exe` (at any depth) to `dest`.
fn extract_exe(zip: &Path, dest: &Path) -> Result<()> {
    let mut archive =
        zip::ZipArchive::new(std::fs::File::open(zip)?).context("the downloaded zip is damaged")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let is_exe = entry
            .enclosed_name()
            .and_then(|p| p.file_name().map(|n| n.eq_ignore_ascii_case(PORTABLE_EXE)))
            .unwrap_or(false);
        if is_exe && entry.is_file() {
            let mut out = std::fs::File::create(dest)?;
            std::io::copy(&mut entry, &mut out)?;
            return Ok(());
        }
    }
    bail!("the zip has no {PORTABLE_EXE}")
}

/// Moves `new` into the running exe's place: the running one is renamed to `.old` first
/// (allowed on Windows) and put back if the move fails.
fn swap_running_exe(exe: &Path, new: &Path) -> Result<()> {
    let old = sibling(exe, ".old")?;
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).context("could not set the running version aside")?;
    if let Err(e) = std::fs::rename(new, exe) {
        let _ = std::fs::rename(&old, exe);
        return Err(e).context("could not put the new version in place");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flatpak_command() {
        let args = flatpak_install_args(Path::new("/c/b.flatpak"), false);
        assert_eq!(
            args,
            [
                "install",
                "--user",
                "-y",
                "--reinstall",
                "--bundle",
                "/c/b.flatpak"
            ]
            .map(OsString::from)
        );
        let system = flatpak_install_args(Path::new("/c/b.flatpak"), true);
        assert_eq!(system[1], "--system");
    }

    #[test]
    fn msi_command() {
        assert_eq!(
            msi_args(Path::new("C:/t/a.msi")),
            ["/i", "C:/t/a.msi", "/passive"].map(OsString::from)
        );
    }

    #[test]
    fn other_kinds_and_missing_packages_are_refused() {
        let release = Release::placeholder(super::super::Version(9, 0, 0));
        let progress = AtomicU32::new(0);
        assert!(prepare(&PackageKind::Other, &release, &progress).is_err());
        let err = prepare(&PackageKind::Msi, &release, &progress).unwrap_err();
        assert!(err.to_string().contains("no package"));
    }

    #[test]
    fn portable_swap_and_extract() {
        let dir = tempfile::tempdir().unwrap();
        let zip_path = dir.path().join("p.zip");
        {
            let mut w = zip::ZipWriter::new(std::fs::File::create(&zip_path).unwrap());
            let opts = zip::write::SimpleFileOptions::default();
            w.start_file("readme.txt", opts).unwrap();
            std::io::Write::write_all(&mut w, b"hi").unwrap();
            w.start_file("3D BuildCraft/buildcraft3d.exe", opts)
                .unwrap();
            std::io::Write::write_all(&mut w, b"new").unwrap();
            w.finish().unwrap();
        }
        let exe = dir.path().join("buildcraft3d.exe");
        std::fs::write(&exe, b"old").unwrap();
        let new = sibling(&exe, ".new").unwrap();
        extract_exe(&zip_path, &new).unwrap();
        swap_running_exe(&exe, &new).unwrap();
        assert_eq!(std::fs::read(&exe).unwrap(), b"new");
        assert_eq!(
            std::fs::read(dir.path().join("buildcraft3d.exe.old")).unwrap(),
            b"old"
        );
        assert!(!new.exists());
    }
}
