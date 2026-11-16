//! Talking to GitHub: the release check and the package download. Every URL, redirects
//! included, must pass [`super::allowed_url`].

use super::{allowed_url, checksum_in, newest_after, parse_releases, Asset, Release, Version};
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

/// Redirects followed before giving up (GitHub uses one for assets).
const MAX_REDIRECTS: usize = 5;
/// No package is this large; a bigger answer is refused.
const MAX_DOWNLOAD: u64 = 512 * 1024 * 1024;
/// Progress is counted in thousandths.
pub const PROGRESS_STEPS: u32 = 1000;

/// Identifies the app to GitHub, which asks API clients for one; no personal data.
fn user_agent() -> String {
    format!("3D-BuildCraft/{}", env!("CARGO_PKG_VERSION"))
}

/// An HTTP client that does not follow redirects itself, so each hop is checked.
fn client(timeout: Option<Duration>) -> ureq::Agent {
    ureq::Agent::config_builder()
        .https_only(true)
        .max_redirects(0)
        .http_status_as_error(false)
        .timeout_global(timeout)
        .timeout_connect(Some(Duration::from_secs(10)))
        .timeout_per_read(Some(Duration::from_secs(30)))
        .user_agent(user_agent())
        .build()
        .into()
}

/// GETs `url`, following redirects to allowed hosts only; errors on other statuses.
fn get(agent: &ureq::Agent, url: &str) -> Result<ureq::http::Response<ureq::Body>> {
    let mut url = url.to_owned();
    for _ in 0..=MAX_REDIRECTS {
        if !allowed_url(&url) {
            bail!("refused to download from an unexpected address");
        }
        let resp = agent
            .get(&url)
            .header(
                "Accept",
                "application/vnd.github+json, application/octet-stream",
            )
            .call()
            .context("could not reach GitHub")?;
        let status = resp.status();
        if status.is_redirection() {
            url = resp
                .headers()
                .get("location")
                .and_then(|l| l.to_str().ok())
                .context("redirect without a location")?
                .to_owned();
            continue;
        }
        if !status.is_success() {
            bail!("GitHub answered {status}");
        }
        return Ok(resp);
    }
    bail!("too many redirects")
}

/// The newest release above `current`, if there is one.
pub fn check(current: Version) -> Result<Option<Release>> {
    let mut resp = get(&client(Some(Duration::from_secs(15))), super::RELEASES_API)?;
    let body = resp.body_mut().read_to_string()?;
    Ok(newest_after(parse_releases(&body)?, current))
}

/// Downloads `asset` to `dest`, counting thousandths of its size in `progress`.
pub fn download(asset: &Asset, dest: &Path, progress: &AtomicU32) -> Result<()> {
    let mut resp = get(&client(None), &asset.url)?;
    let limit = if asset.size > 0 {
        asset.size
    } else {
        MAX_DOWNLOAD
    };
    let mut reader = resp.body_mut().as_reader();
    let mut file = std::fs::File::create(dest)
        .with_context(|| format!("could not write {}", dest.display()))?;
    let mut buf = vec![0; 64 * 1024];
    let mut total = 0u64;
    loop {
        let n = reader.read(&mut buf).context("the download broke off")?;
        if n == 0 {
            break;
        }
        total += n as u64;
        if total > limit {
            bail!("the download is larger than the release says");
        }
        file.write_all(&buf[..n])?;
        if let Some(done) = (total * u64::from(PROGRESS_STEPS)).checked_div(asset.size) {
            progress.store(done as u32, Ordering::Relaxed);
        }
    }
    file.sync_all()?;
    if asset.size > 0 && total != asset.size {
        bail!("the download is incomplete");
    }
    Ok(())
}

/// Checks `file` against the release's `SHA256SUMS`, when it publishes one. Returns
/// whether it was checked.
pub fn verify(release: &Release, asset: &Asset, file: &Path) -> Result<bool> {
    let Some(sums) = release.asset(super::CHECKSUMS) else {
        log::info!("release {} has no {}", release.version, super::CHECKSUMS);
        return Ok(false);
    };
    let mut resp = get(&client(Some(Duration::from_secs(30))), &sums.url)?;
    let text = resp
        .body_mut()
        .with_config()
        .limit(256 * 1024)
        .read_to_string()?;
    let Some(expected) = checksum_in(&text, &asset.name) else {
        bail!("{} does not list {}", super::CHECKSUMS, asset.name);
    };
    if sha256_file(file)? != expected {
        bail!("the download does not match its checksum");
    }
    Ok(true)
}

/// The file's SHA-256 in lower-case hex.
pub fn sha256_file(path: &Path) -> Result<String> {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    let mut file = std::fs::File::open(path)?;
    std::io::copy(&mut file, &mut hasher)?;
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            sha256_file(&path).unwrap(),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Reads the real release list (network): `cargo test -- --ignored real_release_check`.
    #[test]
    #[ignore]
    fn real_release_check() {
        let found = check(Version(0, 1, 0))
            .unwrap()
            .expect("a release above 0.1.0");
        assert!(found.version > Version(0, 1, 0));
        assert!(found.asset("buildcraft3d-linux-x86_64.flatpak").is_some());
        assert!(found.assets.iter().all(|a| allowed_url(&a.url)));
        // The download follows GitHub's redirect to its asset storage; nothing is run.
        let zip = found.asset("buildcraft3d-windows-x64.zip").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let progress = AtomicU32::new(0);
        download(zip, &dir.path().join("p.zip"), &progress).unwrap();
        assert_eq!(progress.load(Ordering::Relaxed), PROGRESS_STEPS);
    }

    #[test]
    fn refuses_other_hosts_before_connecting() {
        let asset = Asset {
            name: "x".into(),
            url: "http://example.com/x".into(),
            size: 1,
        };
        let dir = tempfile::tempdir().unwrap();
        let err = download(&asset, &dir.path().join("x"), &AtomicU32::new(0)).unwrap_err();
        assert!(err.to_string().contains("unexpected address"));
    }
}
