//! Updates: finds a newer release of the app on GitHub, downloads the package that matches
//! how this copy was installed (Flatpak, AppImage, MSI or the portable zip), puts it in
//! place and restarts.
//!
//! Every release is published as a GitHub pre-release, so `/releases/latest` answers 404:
//! the release list is read instead and the highest `vX.Y.Z` tag wins. This module holds the
//! parts that need no network or UI (versions, picking the release and its package, the
//! host allow-list, the commands run); [`fetch`] talks to GitHub and [`install`] puts the
//! download in place.

pub mod fetch;
pub mod install;

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The release list of the public repository, newest first.
pub const RELEASES_API: &str =
    "https://api.github.com/repos/Nico2433/3D-BuildCraft/releases?per_page=20";
/// Where the releases can be downloaded by hand.
pub const RELEASES_PAGE: &str = "https://github.com/Nico2433/3D-BuildCraft/releases";
/// A release asset listing `<sha256>  <file name>` lines; when present, downloads are checked.
pub const CHECKSUMS: &str = "SHA256SUMS";
/// Pretends this copy is that version (e.g. `0.1.0`), to test the update with a real check.
pub const FAKE_VERSION_ENV: &str = "BUILDCRAFT3D_FAKE_VERSION";
/// The Flatpak app id, when `FLATPAK_ID` is not set.
const FLATPAK_APP_ID: &str = "io.github.nico2433.BuildCraft3D";
/// Hosts the check and downloads may reach: the API, the release pages and the asset
/// storage GitHub redirects downloads to.
const ALLOWED_HOSTS: [&str; 4] = [
    "api.github.com",
    "github.com",
    "objects.githubusercontent.com",
    "release-assets.githubusercontent.com",
];

/// A release version, `major.minor.patch`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u64, pub u64, pub u64);

impl Version {
    /// Reads `vX.Y.Z` or `X.Y.Z`; anything else (suffixes such as `-rc1` included) is None.
    pub fn parse(text: &str) -> Option<Version> {
        let text = text.trim();
        let text = text.strip_prefix(['v', 'V']).unwrap_or(text);
        let mut parts = text.split('.').map(|p| {
            (!p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
                .then(|| p.parse::<u64>().ok())
                .flatten()
        });
        let v = Version(parts.next()??, parts.next()??, parts.next()??);
        parts.next().is_none().then_some(v)
    }

    /// This build's version, or the one [`FAKE_VERSION_ENV`] gives.
    pub fn current() -> Version {
        std::env::var(FAKE_VERSION_ENV)
            .ok()
            .and_then(|v| Version::parse(&v))
            .or_else(|| Version::parse(env!("CARGO_PKG_VERSION")))
            .unwrap_or(Version(0, 0, 0))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

/// A file attached to a release.
#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    /// Bytes, as GitHub reports it; 0 when unknown.
    pub size: u64,
}

/// A published release.
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    /// The release's page on GitHub.
    pub page: String,
    pub assets: Vec<Asset>,
}

impl Release {
    /// A release with no files, for screenshots (`--fake-update`).
    pub fn placeholder(version: Version) -> Release {
        Release {
            version,
            page: RELEASES_PAGE.to_owned(),
            assets: Vec::new(),
        }
    }

    pub fn asset(&self, name: &str) -> Option<&Asset> {
        self.assets.iter().find(|a| a.name == name)
    }
}

#[derive(Deserialize)]
struct RawRelease {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    html_url: String,
    #[serde(default)]
    assets: Vec<RawAsset>,
}

#[derive(Deserialize)]
struct RawAsset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// The releases in GitHub's JSON list, leaving out drafts and tags that are not versions.
pub fn parse_releases(json: &str) -> Result<Vec<Release>> {
    let raw: Vec<RawRelease> =
        serde_json::from_str(json).context("unexpected answer from GitHub")?;
    Ok(raw
        .into_iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            Some(Release {
                version: Version::parse(&r.tag_name)?,
                page: r.html_url,
                assets: r
                    .assets
                    .into_iter()
                    .map(|a| Asset {
                        name: a.name,
                        url: a.browser_download_url,
                        size: a.size,
                    })
                    .collect(),
            })
        })
        .collect())
}

/// The highest release above `current`, if any; the list may be in any order.
pub fn newest_after(releases: Vec<Release>, current: Version) -> Option<Release> {
    releases
        .into_iter()
        .filter(|r| r.version > current)
        .max_by_key(|r| r.version)
}

/// How this copy was installed, which decides how it updates.
#[derive(Clone, Debug, PartialEq)]
pub enum PackageKind {
    /// A Flatpak; `system` when installed for all users rather than with `--user`.
    Flatpak { system: bool },
    /// An AppImage at this path.
    AppImage(PathBuf),
    /// Installed on Windows with the MSI.
    Msi,
    /// The portable Windows exe from the zip, at this path.
    Portable(PathBuf),
    /// Anything else (a development build, macOS): the release page opens instead.
    Other,
}

impl PackageKind {
    /// Looks at the sandbox, the environment and where the exe is.
    pub fn detect() -> PackageKind {
        if crate::host::in_flatpak() {
            let info = std::fs::read_to_string("/.flatpak-info").unwrap_or_default();
            return PackageKind::Flatpak {
                system: flatpak_is_system(&info),
            };
        }
        if let Some(path) = std::env::var_os("APPIMAGE").map(PathBuf::from) {
            if path.is_file() {
                return PackageKind::AppImage(path);
            }
        }
        if cfg!(windows) && !cfg!(debug_assertions) {
            if let Ok(exe) = std::env::current_exe() {
                let program_files: Vec<PathBuf> = ["ProgramFiles", "ProgramW6432"]
                    .iter()
                    .filter_map(std::env::var_os)
                    .map(PathBuf::from)
                    .collect();
                let writable = exe.parent().is_some_and(dir_writable);
                return windows_kind(&exe, &program_files, writable);
            }
        }
        PackageKind::Other
    }

    /// The release file this kind installs from; None when it cannot update itself.
    pub fn asset_name(&self) -> Option<&'static str> {
        match self {
            PackageKind::Flatpak { .. } => Some("buildcraft3d-linux-x86_64.flatpak"),
            PackageKind::AppImage(_) => Some("buildcraft3d-linux-x86_64.AppImage"),
            PackageKind::Msi => Some("buildcraft3d-windows-x64.msi"),
            PackageKind::Portable(_) => Some("buildcraft3d-windows-x64.zip"),
            PackageKind::Other => None,
        }
    }

    /// Whether the app can install the update itself.
    pub fn installs(&self) -> bool {
        self.asset_name().is_some()
    }
}

/// The release's file for `kind`, if it has one.
pub fn asset_for<'a>(release: &'a Release, kind: &PackageKind) -> Option<&'a Asset> {
    release.asset(kind.asset_name()?)
}

/// Whether the Flatpak was installed system-wide, from `/.flatpak-info`'s `app-path`:
/// per-user installations live under `~/.local/share/flatpak`.
fn flatpak_is_system(info: &str) -> bool {
    info.lines()
        .find_map(|l| l.trim().strip_prefix("app-path="))
        .is_some_and(|p| !p.contains("/.local/share/flatpak/"))
}

/// The Flatpak's app id.
fn flatpak_app_id() -> String {
    std::env::var("FLATPAK_ID").unwrap_or_else(|_| FLATPAK_APP_ID.to_owned())
}

/// A Windows exe under Program Files came from the MSI; one in a folder the user can write
/// to is the portable zip's, unless it is a cargo build (`target/<profile>/`).
fn windows_kind(exe: &Path, program_files: &[PathBuf], writable: bool) -> PackageKind {
    let lower = |p: &Path| p.to_string_lossy().to_lowercase();
    if program_files
        .iter()
        .any(|pf| lower(exe).starts_with(&lower(pf)))
    {
        return PackageKind::Msi;
    }
    let in_target = exe
        .parent()
        .and_then(Path::parent)
        .and_then(Path::file_name)
        .is_some_and(|n| n == "target");
    if writable && !in_target {
        PackageKind::Portable(exe.to_path_buf())
    } else {
        PackageKind::Other
    }
}

fn dir_writable(dir: &Path) -> bool {
    let probe = dir.join(".buildcraft3d-write-test");
    let ok = std::fs::write(&probe, b"").is_ok();
    let _ = std::fs::remove_file(probe);
    ok
}

/// Opens a GitHub page in the default browser, without waiting for it.
pub fn open_page(url: &str) -> Result<()> {
    if !allowed_url(url) {
        anyhow::bail!("not a GitHub address");
    }
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    let mut child = crate::host::host_command(program)
        .arg(url)
        .spawn()
        .with_context(|| format!("could not start {program}"))?;
    std::thread::spawn(move || child.wait());
    Ok(())
}

/// The host of an `https://` URL; None for other schemes or URLs with a user part.
pub fn https_host(url: &str) -> Option<&str> {
    let rest = url.strip_prefix("https://")?;
    let authority = rest.split(['/', '?', '#']).next()?;
    if authority.contains('@') {
        return None;
    }
    let host = authority.split(':').next()?;
    (!host.is_empty()).then_some(host)
}

/// Whether the update may fetch `url`: HTTPS on GitHub's hosts only.
pub fn allowed_url(url: &str) -> bool {
    https_host(url).is_some_and(|h| ALLOWED_HOSTS.iter().any(|a| a.eq_ignore_ascii_case(h)))
}

/// The SHA-256 a `SHA256SUMS` file lists for `name` (`<hex>  <name>`, `*name` for binary
/// mode), in lower case.
pub fn checksum_in(sums: &str, name: &str) -> Option<String> {
    sums.lines().find_map(|line| {
        let (hash, file) = line.trim().split_once(char::is_whitespace)?;
        let file = file.trim_start().trim_start_matches('*');
        (file == name && hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
            .then(|| hash.to_ascii_lowercase())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(tag: &str) -> Release {
        Release::placeholder(Version::parse(tag).unwrap())
    }

    #[test]
    fn versions_parse_and_compare() {
        assert_eq!(Version::parse("v0.1.13"), Some(Version(0, 1, 13)));
        assert_eq!(Version::parse("0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(Version::parse(" V1.0.0 "), Some(Version(1, 0, 0)));
        for bad in [
            "",
            "v",
            "0.1",
            "0.1.2.3",
            "v0.1.x",
            "v0.1.2-rc1",
            "nightly",
            "v-1.0.0",
        ] {
            assert_eq!(Version::parse(bad), None, "{bad}");
        }
        assert!(Version(0, 1, 13) < Version(0, 1, 14));
        assert!(Version(0, 1, 99) < Version(0, 2, 0));
        assert!(Version(0, 10, 0) > Version(0, 9, 9));
        assert_eq!(Version(0, 1, 13).to_string(), "0.1.13");
    }

    #[test]
    fn newest_is_the_highest_above_current() {
        let list = vec![
            release("v0.1.12"),
            release("v0.1.15"),
            release("v0.1.9"),
            release("v0.1.14"),
        ];
        let current = Version(0, 1, 13);
        assert_eq!(
            newest_after(list.clone(), current).map(|r| r.version),
            Some(Version(0, 1, 15))
        );
        assert_eq!(newest_after(list, Version(0, 1, 15)), None);
        assert_eq!(newest_after(Vec::new(), current), None);
    }

    #[test]
    fn drafts_and_odd_tags_are_ignored() {
        let json = r#"[
            {"tag_name": "v0.2.0", "draft": true, "html_url": "x", "assets": []},
            {"tag_name": "latest", "draft": false, "html_url": "x", "assets": []},
            {"tag_name": "v0.1.14-rc1", "draft": false, "assets": []},
            {"tag_name": "v0.1.14", "draft": false, "prerelease": true,
             "html_url": "https://github.com/Nico2433/3D-BuildCraft/releases/tag/v0.1.14",
             "assets": [{"name": "buildcraft3d-windows-x64.msi", "size": 10,
                         "browser_download_url": "https://github.com/a/b/c.msi"}]}
        ]"#;
        let releases = parse_releases(json).unwrap();
        assert_eq!(releases.len(), 1);
        let r = &releases[0];
        assert_eq!(r.version, Version(0, 1, 14));
        assert_eq!(r.assets[0].size, 10);
        assert!(parse_releases("{\"message\": \"Not Found\"}").is_err());
    }

    #[test]
    fn each_kind_takes_its_package() {
        let mut r = release("v0.1.14");
        for name in [
            "buildcraft3d-linux-x86_64.AppImage",
            "buildcraft3d-linux-x86_64.flatpak",
            "buildcraft3d-windows-x64.msi",
            "buildcraft3d-windows-x64.zip",
        ] {
            r.assets.push(Asset {
                name: name.into(),
                url: format!("https://github.com/x/{name}"),
                size: 1,
            });
        }
        let name = |r: &Release, k: PackageKind| asset_for(r, &k).map(|a| a.name.clone());
        assert_eq!(
            name(&r, PackageKind::Flatpak { system: false }),
            Some("buildcraft3d-linux-x86_64.flatpak".into())
        );
        assert_eq!(
            name(&r, PackageKind::AppImage("/a".into())),
            Some("buildcraft3d-linux-x86_64.AppImage".into())
        );
        assert_eq!(
            name(&r, PackageKind::Msi),
            Some("buildcraft3d-windows-x64.msi".into())
        );
        assert_eq!(
            name(&r, PackageKind::Portable("b".into())),
            Some("buildcraft3d-windows-x64.zip".into())
        );
        assert_eq!(name(&r, PackageKind::Other), None);
        r.assets.retain(|a| !a.name.ends_with(".msi"));
        assert_eq!(name(&r, PackageKind::Msi), None);
    }

    #[test]
    fn only_github_over_https() {
        assert!(allowed_url(RELEASES_API));
        assert!(allowed_url(
            "https://github.com/Nico2433/3D-BuildCraft/releases/download/v1.0.0/a.msi"
        ));
        assert!(allowed_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/1?sig=x"
        ));
        assert!(allowed_url("https://objects.githubusercontent.com/x"));
        for bad in [
            "http://github.com/x",
            "https://github.com.evil.example/x",
            "https://evil.example/github.com",
            "https://github.com@evil.example/x",
            "https://evilgithub.com/x",
            "file:///etc/passwd",
            "https://",
        ] {
            assert!(!allowed_url(bad), "{bad}");
        }
        assert_eq!(https_host("https://github.com:443/x"), Some("github.com"));
    }

    #[test]
    fn flatpak_installation_scope() {
        let user = "[Application]\nname=x\n\n[Instance]\napp-path=/home/u/.local/share/flatpak/app/x/current/active/files\n";
        let system = "[Instance]\napp-path=/var/lib/flatpak/app/x/x86_64/master/abc/files\n";
        assert!(!flatpak_is_system(user));
        assert!(flatpak_is_system(system));
        assert!(!flatpak_is_system(""));
    }

    #[test]
    fn windows_install_kind() {
        let pf = vec![PathBuf::from(r"C:\Program Files")];
        let msi = Path::new(r"C:\Program Files\3D BuildCraft\buildcraft3d.exe");
        assert_eq!(windows_kind(msi, &pf, false), PackageKind::Msi);
        let portable = Path::new(r"D:\Tools\BuildCraft\buildcraft3d.exe");
        assert_eq!(
            windows_kind(portable, &pf, true),
            PackageKind::Portable(portable.to_path_buf())
        );
        assert_eq!(windows_kind(portable, &pf, false), PackageKind::Other);
        let cargo = Path::new("/src/app/target/release/buildcraft3d.exe");
        assert_eq!(windows_kind(cargo, &pf, true), PackageKind::Other);
    }

    #[test]
    fn checksum_lines() {
        let a = "a".repeat(64);
        let b = "B".repeat(64);
        let sums = format!(
            "{a}  buildcraft3d-windows-x64.msi\n{b} *buildcraft3d-linux-x86_64.flatpak\nbad line\n"
        );
        assert_eq!(
            checksum_in(&sums, "buildcraft3d-windows-x64.msi"),
            Some(a.clone())
        );
        assert_eq!(
            checksum_in(&sums, "buildcraft3d-linux-x86_64.flatpak"),
            Some("b".repeat(64))
        );
        assert_eq!(checksum_in(&sums, "buildcraft3d-windows-x64.zip"), None);
        assert_eq!(checksum_in("abc  x", "x"), None);
    }
}
