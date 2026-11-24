//! Automatic updates from GitHub Releases.
//!
//! A background thread asks GitHub for the latest release at start and every few hours.
//! When it is newer than this build, the `*-windows-x64.exe` asset is downloaded next to
//! the running exe as `<exe>.update`, checked (size, SHA-256 digest when GitHub gives one,
//! PE header) and reported as ready. It is swapped in when the person clicks "Reiniciar"
//! or when the app closes: Windows lets a running exe be renamed, so the running one
//! becomes `<exe>.old` (deleted on the next start) and the new one takes its name.

use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;

pub const REPO: &str = "gabriel20vieira/LegionMonitorControlCenter";
const ASSET_SUFFIX: &str = "-windows-x64.exe";
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const MAX_SIZE: u64 = 200 * 1024 * 1024;

pub enum Event {
    /// A newer version was downloaded and checked; it is waiting at `staged`.
    Ready { version: String, staged: PathBuf },
}

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    prerelease: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    size: u64,
    /// "sha256:<hex>", set by GitHub on newer uploads.
    #[serde(default)]
    digest: Option<String>,
}

pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

fn parse(v: &str) -> Option<(u64, u64, u64)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let core = v.split(['-', '+']).next()?;
    let mut it = core.split('.');
    let major = it.next()?.parse().ok()?;
    let minor = it.next().unwrap_or("0").parse().ok()?;
    let patch = it.next().unwrap_or("0").parse().ok()?;
    Some((major, minor, patch))
}

/// True when `latest` is a higher version than `current` ("v0.2.0" > "0.1.9").
pub fn is_newer(latest: &str, current: &str) -> bool {
    matches!((parse(latest), parse(current)), (Some(a), Some(b)) if a > b)
}

fn sibling(exe: &Path, suffix: &str) -> PathBuf {
    let mut name = exe.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    exe.with_file_name(name)
}

/// Starts the checker thread. Nothing is sent when there is no update or the check fails.
pub fn spawn(ctx: eframe::egui::Context) -> Receiver<Event> {
    let (tx, rx) = channel();
    std::thread::Builder::new()
        .name("update".into())
        .spawn(move || {
            cleanup_old();
            loop {
                match check_and_stage() {
                    Ok(Some((version, staged))) => {
                        let _ = tx.send(Event::Ready { version, staged });
                        ctx.request_repaint();
                        return;
                    }
                    Ok(None) => {}
                    Err(e) => eprintln!("update check failed: {e}"),
                }
                std::thread::sleep(CHECK_EVERY);
            }
        })
        .expect("update thread");
    rx
}

/// Removes the exe left behind by the previous update. The old process may still be
/// closing, so this retries for a few seconds.
fn cleanup_old() {
    let Ok(exe) = std::env::current_exe() else { return };
    let old = sibling(&exe, ".old");
    for _ in 0..10 {
        if !old.exists() || std::fs::remove_file(&old).is_ok() {
            break;
        }
        std::thread::sleep(Duration::from_secs(1));
    }
    let _ = std::fs::remove_file(sibling(&exe, ".update"));
}

fn check_and_stage() -> Result<Option<(String, PathBuf)>, String> {
    let agent = ureq::AgentBuilder::new()
        .timeout_connect(Duration::from_secs(15))
        .timeout(Duration::from_secs(300))
        .user_agent(concat!("monitor-control-center/", env!("CARGO_PKG_VERSION")))
        .build();
    let release: Release = agent
        .get(&format!("https://api.github.com/repos/{REPO}/releases/latest"))
        .set("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?
        .into_json()
        .map_err(|e| e.to_string())?;
    if release.draft || release.prerelease || !is_newer(&release.tag_name, current_version()) {
        return Ok(None);
    }
    let asset = release
        .assets
        .iter()
        .find(|a| a.name.ends_with(ASSET_SUFFIX))
        .ok_or_else(|| format!("release {} has no *{ASSET_SUFFIX} asset", release.tag_name))?;
    if asset.size == 0 || asset.size > MAX_SIZE {
        return Err(format!("unexpected asset size {}", asset.size));
    }

    let mut bytes = Vec::with_capacity(asset.size as usize);
    agent
        .get(&asset.browser_download_url)
        .call()
        .map_err(|e| e.to_string())?
        .into_reader()
        .take(asset.size + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    verify(&bytes, asset.size, asset.digest.as_deref())?;

    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let staged = sibling(&exe, ".update");
    std::fs::write(&staged, &bytes).map_err(|e| format!("cannot write {}: {e}", staged.display()))?;
    Ok(Some((release.tag_name.trim_start_matches(['v', 'V']).to_string(), staged)))
}

fn verify(bytes: &[u8], size: u64, digest: Option<&str>) -> Result<(), String> {
    if bytes.len() as u64 != size {
        return Err(format!("downloaded {} bytes, expected {size}", bytes.len()));
    }
    if !bytes.starts_with(b"MZ") {
        return Err("download is not a Windows executable".into());
    }
    if let Some(expected) = digest.and_then(|d| d.strip_prefix("sha256:")) {
        let got: String = Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect();
        if !got.eq_ignore_ascii_case(expected) {
            return Err("SHA-256 does not match the release".into());
        }
    }
    Ok(())
}

/// Puts the staged exe in place of the running one. Returns the exe path to start.
pub fn install(staged: &Path) -> std::io::Result<PathBuf> {
    let exe = std::env::current_exe()?;
    let old = sibling(&exe, ".old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(&exe, &old)?;
    if let Err(e) = std::fs::rename(staged, &exe) {
        let _ = std::fs::rename(&old, &exe);
        return Err(e);
    }
    Ok(exe)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(is_newer("v0.2.0", "0.1.9"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(is_newer("0.1.10", "0.1.9"));
        assert!(!is_newer("v0.1.2", "0.1.2"));
        assert!(!is_newer("v0.1.1", "0.1.2"));
        assert!(!is_newer("nightly", "0.1.2"));
        assert!(is_newer("v0.3", "0.2.5"));
        assert!(!is_newer("v0.2.0-beta.1", "0.2.0"));
    }

    #[test]
    fn checks_download() {
        let body = b"MZ rest of the file";
        let sha: String = Sha256::digest(body).iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify(body, body.len() as u64, Some(&format!("sha256:{sha}"))).is_ok());
        assert!(verify(body, body.len() as u64, None).is_ok());
        assert!(verify(body, body.len() as u64 + 1, None).is_err());
        assert!(verify(body, body.len() as u64, Some("sha256:00")).is_err());
        assert!(verify(b"PK zip", 6, None).is_err());
    }
}
