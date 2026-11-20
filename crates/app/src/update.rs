//! Version information and self-update from the GitHub release "latest".
//!
//! CI stamps each Windows build with `BUILD_DOCTOR_VERSION` (`0.1.<build>`)
//! and `BUILD_DOCTOR_COMMIT`, and publishes the newest `main` build as the
//! repository's latest release, next to a `version.json` naming its version
//! and the SHA-256 of each executable. Updating downloads the app (and the
//! command-line `doctor.exe` when it sits next to the app, as in the
//! "Build Doctor" folder), checks every hash before changing anything, swaps
//! them in and restarts.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

const RELEASE: &str = "https://github.com/tragiik1/build-doctor/releases/latest/download/";
const APP_ASSET: &str = "build-doctor.exe";
const CLI_ASSET: &str = "doctor.exe";

/// This build's version: `0.1.<build>` for CI builds, `<crate version>-dev`
/// for local ones.
pub fn version() -> String {
    option_env!("BUILD_DOCTOR_VERSION").map_or_else(
        || format!("{}-dev", env!("CARGO_PKG_VERSION")),
        str::to_owned,
    )
}

/// The commit this build was made from, when CI built it.
pub fn commit() -> Option<&'static str> {
    option_env!("BUILD_DOCTOR_COMMIT")
}

/// The build number in a version like `0.1.17`.
fn build_number(version: &str) -> Option<u64> {
    version.rsplit('.').next()?.parse().ok()
}

/// What `version.json` in the latest release says.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Release {
    pub version: String,
    pub commit: String,
    /// The app's SHA-256.
    pub sha256: String,
    /// `doctor.exe`'s SHA-256 (releases since the folder layout).
    #[serde(default)]
    pub doctor_sha256: Option<String>,
}

fn agent(timeout: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .user_agent(concat!("build-doctor/", env!("CARGO_PKG_VERSION")))
        .timeout_connect(Some(Duration::from_secs(15)))
        .timeout_global(Some(timeout))
        .build()
        .into()
}

/// The latest release, if it is newer than this build. Local builds (no
/// build number) never offer updates.
pub fn check() -> Result<Option<Release>, String> {
    let Some(current) = build_number(&version()) else {
        return Ok(None);
    };
    let body = agent(Duration::from_secs(20))
        .get(&format!("{RELEASE}version.json"))
        .call()
        .map_err(|e| format!("Could not check for updates: {e}"))?
        .into_body()
        .read_to_vec()
        .map_err(|e| format!("Could not check for updates: {e}"))?;
    let release: Release = serde_json::from_slice(&body)
        .map_err(|e| format!("Could not read the update information: {e}"))?;
    Ok(build_number(&release.version)
        .filter(|latest| *latest > current)
        .map(|_| release))
}

/// Downloads the release's executables, checks each SHA-256, and puts them
/// in place of the running ones (kept as `.old` until the next start).
/// Nothing changes unless every download matches. `progress` gets (bytes
/// so far, total bytes if known) for each download.
pub fn install(release: &Release, progress: &dyn Fn(u64, Option<u64>)) -> Result<(), String> {
    let app =
        std::env::current_exe().map_err(|e| format!("Cannot find the running program: {e}"))?;
    let mut wanted = vec![(app.clone(), APP_ASSET, release.sha256.as_str())];
    let cli = app.with_file_name(CLI_ASSET);
    if let (true, Some(sha)) = (cli.is_file(), release.doctor_sha256.as_deref()) {
        wanted.push((cli, CLI_ASSET, sha));
    }
    let mut ready = Vec::new();
    for (path, asset, sha) in wanted {
        let bytes = download(asset, sha, progress)?;
        let new = with_suffix(&path, "new");
        fs::write(&new, &bytes).map_err(|e| format!("Cannot write {}: {e}", new.display()))?;
        ready.push(path);
    }
    let mut swapped: Vec<&PathBuf> = Vec::new();
    for path in &ready {
        if let Err(err) = swap(path) {
            for done in swapped {
                restore(done);
            }
            return Err(err);
        }
        swapped.push(path);
    }
    Ok(())
}

/// Downloads a release asset and checks it against `sha256`.
fn download(
    asset: &str,
    sha256: &str,
    progress: &dyn Fn(u64, Option<u64>),
) -> Result<Vec<u8>, String> {
    let response = agent(Duration::from_secs(15 * 60))
        .get(&format!("{RELEASE}{asset}"))
        .call()
        .map_err(|e| format!("Download failed: {e}"))?;
    let total = response.body().content_length();
    let mut reader = response.into_body().into_reader();
    let mut bytes = Vec::with_capacity(total.unwrap_or(8 << 20) as usize);
    let mut chunk = [0u8; 64 * 1024];
    loop {
        let n = reader
            .read(&mut chunk)
            .map_err(|e| format!("Download failed: {e}"))?;
        if n == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..n]);
        progress(bytes.len() as u64, total);
    }
    let actual = hex(&Sha256::digest(&bytes));
    if !actual.eq_ignore_ascii_case(sha256) {
        return Err(format!(
            "The download of {asset} does not match the release (SHA-256 {actual}, expected {sha256}). Nothing was changed."
        ));
    }
    Ok(bytes)
}

/// Puts `path.new` in place of `path`, keeping `path.old`. Windows lets a
/// running program be renamed, not replaced.
fn swap(path: &Path) -> Result<(), String> {
    let (new, old) = (with_suffix(path, "new"), with_suffix(path, "old"));
    let _ = fs::remove_file(&old);
    fs::rename(path, &old).map_err(|e| format!("Cannot move {} aside: {e}", path.display()))?;
    if let Err(e) = fs::rename(&new, path) {
        let _ = fs::rename(&old, path);
        return Err(format!(
            "Cannot put the new {} in place: {e}",
            path.display()
        ));
    }
    Ok(())
}

/// Undoes a [`swap`].
fn restore(path: &Path) {
    let _ = fs::remove_file(path);
    let _ = fs::rename(with_suffix(path, "old"), path);
}

/// Removes the previous executables an update left behind.
pub fn clean_up() {
    if let Ok(app) = std::env::current_exe() {
        for path in [app.clone(), app.with_file_name(CLI_ASSET)] {
            let _ = fs::remove_file(with_suffix(&path, "old"));
            let _ = fs::remove_file(with_suffix(&path, "new"));
        }
    }
}

fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_owned();
    name.push(format!(".{suffix}"));
    path.with_file_name(name)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_build_numbers() {
        assert_eq!(build_number("0.1.17"), Some(17));
        assert_eq!(build_number("0.1.0-dev"), None);
    }

    #[test]
    fn names_side_files() {
        assert_eq!(
            with_suffix(Path::new("C:/x/build-doctor.exe"), "old"),
            Path::new("C:/x/build-doctor.exe.old")
        );
    }

    #[test]
    fn reads_old_and_new_version_files() {
        let old: Release =
            serde_json::from_str(r#"{"version":"0.1.30","commit":"abc","sha256":"00"}"#).unwrap();
        assert_eq!(old.doctor_sha256, None);
        let new: Release = serde_json::from_str(
            r#"{"version":"0.1.40","commit":"abc","sha256":"00","doctorSha256":"11"}"#,
        )
        .unwrap();
        assert_eq!(new.doctor_sha256.as_deref(), Some("11"));
    }

    #[test]
    fn swaps_and_restores() {
        let dir = std::env::temp_dir().join(format!("bd-swap-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let exe = dir.join("doctor.exe");
        fs::write(&exe, "old").unwrap();
        fs::write(with_suffix(&exe, "new"), "new").unwrap();
        swap(&exe).unwrap();
        assert_eq!(fs::read_to_string(&exe).unwrap(), "new");
        assert_eq!(fs::read_to_string(with_suffix(&exe, "old")).unwrap(), "old");
        restore(&exe);
        assert_eq!(fs::read_to_string(&exe).unwrap(), "old");
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn hashes_like_sha256sum() {
        assert_eq!(
            hex(&Sha256::digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
