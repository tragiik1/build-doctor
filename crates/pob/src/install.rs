//! A verified copy of one PoB2 release on disk.
//!
//! Layout of an install directory (mirrors the PoB2 repository, minus art):
//!
//! ```text
//! <root>/manifest.xml     PoB's own file list with SHA-1 per file
//! <root>/src/...          calculation code and game data
//! <root>/runtime/lua/...  pure-Lua libraries PoB requires
//! ```
//!
//! Integrity uses PoB's own `manifest.xml`, the same list the PoB app's
//! updater trusts: every kept file must match its SHA-1, computed the way
//! PoB's `update_manifest.py` does (text files hashed with CRLF line endings).

use std::collections::VecDeque;
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::time::Duration;

use crate::{Error, Result};

const REPOSITORY: &str = "PathOfBuildingCommunity/PathOfBuilding-PoE2";

/// Files the engine needs that PoB's manifest deliberately leaves out (its
/// installer excludes them; see PoB2's `manifest.cfg`). They are taken from
/// the same archive and their SHA-1 is recorded in [`UNLISTED_RECORD`] at
/// install time, so `verify` still detects later changes. The first one is
/// required; the rest are extracted when the release has them.
const UNLISTED_FILES: &[&str] = &["src/HeadlessWrapper.lua", "src/_SimpleGraphic.def.lua"];

/// `<sha1>  <path>` lines for [`UNLISTED_FILES`], written by `download`.
const UNLISTED_RECORD: &str = "unlisted.sha1";

/// Images and textures are only used for drawing; the calculation never reads them.
const SKIPPED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "dds", "zst", "ttf", "ico"];

/// A PoB2 release number as written in its manifest, e.g. `0.23.1`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct PobVersion(String);

impl PobVersion {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The git tag PoB2 uses for this release.
    pub fn tag(&self) -> String {
        format!("v{}", self.0)
    }
}

impl fmt::Display for PobVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[derive(Debug, Clone)]
pub struct Install {
    root: PathBuf,
    version: PobVersion,
}

/// One file PoB's manifest lists, mapped to where it lives in an install.
#[derive(Debug)]
struct ManifestFile {
    path: String,
    sha1: String,
}

struct Manifest {
    version: PobVersion,
    files: Vec<ManifestFile>,
}

impl Install {
    /// Opens an existing install or PoB2 checkout without re-hashing it.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        let manifest = read_file(&root.join("manifest.xml"))?;
        let version = parse_manifest(&manifest)?.version;
        for required in [
            "src/HeadlessWrapper.lua",
            "src/Launch.lua",
            "runtime/lua/xml.lua",
        ] {
            if !root.join(required).is_file() {
                return Err(Error::NotAnInstall(format!(
                    "{} has no {required}",
                    root.display()
                )));
            }
        }
        Ok(Self { root, version })
    }

    /// Re-hashes every kept file against the manifest, plus the unlisted
    /// engine files against the hashes recorded when they were installed.
    pub fn verify(&self) -> Result<()> {
        let manifest = parse_manifest(&read_file(&self.root.join("manifest.xml"))?)?;
        let mut files = manifest.files;
        let record = self.root.join(UNLISTED_RECORD);
        if record.is_file() {
            files.extend(parse_unlisted_record(&read_file(&record)?)?);
        }
        for file in &files {
            let path = self.root.join(&file.path);
            let bytes = fs::read(&path).map_err(|e| Error::io("read", &path, e))?;
            check_hash(&file.path, &bytes, &file.sha1)?;
        }
        Ok(())
    }

    /// Downloads PoB2 release `tag` from GitHub into `<parent>/<version>/`
    /// and returns the install. Fetches only the files the engine uses
    /// (about 1,100 files, ~50 MB), several at a time, each checked against
    /// PoB's manifest; `progress` is told after every file. An existing
    /// install of the same version is replaced only after the new one
    /// verifies.
    pub fn download(
        tag: &str,
        parent: &Path,
        progress: &(dyn Fn(InstallProgress) + Sync),
    ) -> Result<Self> {
        remove_stale_archives(parent);
        let base = format!("https://raw.githubusercontent.com/{REPOSITORY}/{tag}/");
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(concat!("build-doctor/", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(20)))
            .timeout_global(Some(Duration::from_secs(180)))
            .http_status_as_error(false)
            .build()
            .into();
        let fetch = |path: &str| -> Result<Option<Vec<u8>>> {
            let url = format!("{base}{}", url_path(path));
            let mut last = String::new();
            for attempt in 0..3 {
                if attempt > 0 {
                    std::thread::sleep(Duration::from_secs(attempt));
                }
                match agent.get(&url).call() {
                    Ok(response) if response.status() == 404 => return Ok(None),
                    Ok(response) if response.status().is_success() => {
                        match response
                            .into_body()
                            .with_config()
                            .limit(64 * 1024 * 1024)
                            .read_to_vec()
                        {
                            Ok(bytes) => return Ok(Some(bytes)),
                            Err(e) => last = e.to_string(),
                        }
                    }
                    Ok(response) => last = format!("HTTP {}", response.status()),
                    Err(e) => last = e.to_string(),
                }
            }
            Err(Error::Download(format!("{url}: {last}")))
        };

        let manifest_bytes = fetch("manifest.xml")?
            .ok_or_else(|| Error::Download(format!("PoB2 {tag} not found on GitHub")))?;
        let manifest_text = String::from_utf8(manifest_bytes)
            .map_err(|_| Error::Corrupt("manifest.xml is not UTF-8".into()))?;
        let manifest = parse_manifest(&manifest_text)?;
        let staging = fresh_staging(parent, &manifest.version)?;

        let total = manifest.files.len() + UNLISTED_FILES.len();
        let done = AtomicUsize::new(0);
        let bytes = AtomicU64::new(0);
        let queue = Mutex::new(manifest.files.iter().collect::<VecDeque<_>>());
        let failure: Mutex<Option<Error>> = Mutex::new(None);
        let report = |size: usize| {
            let done = done.fetch_add(1, Ordering::SeqCst) + 1;
            let bytes = bytes.fetch_add(size as u64, Ordering::SeqCst) + size as u64;
            progress(InstallProgress { done, total, bytes });
        };
        std::thread::scope(|scope| {
            for _ in 0..PARALLEL_DOWNLOADS {
                scope.spawn(|| {
                    loop {
                        if failure.lock().map_or(true, |f| f.is_some()) {
                            return;
                        }
                        let Some(file) = queue.lock().ok().and_then(|mut q| q.pop_front()) else {
                            return;
                        };
                        let result = fetch(&file.path).and_then(|found| {
                            let data = found.ok_or_else(|| {
                                Error::Corrupt(format!("GitHub has no {}", file.path))
                            })?;
                            check_hash(&file.path, &data, &file.sha1)?;
                            write_file(&staging.join(&file.path), &data)?;
                            Ok(data.len())
                        });
                        match result {
                            Ok(size) => report(size),
                            Err(err) => {
                                if let Ok(mut f) = failure.lock() {
                                    f.get_or_insert(err);
                                }
                                return;
                            }
                        }
                    }
                });
            }
        });
        if let Some(err) = failure.into_inner().unwrap_or_default() {
            let _ = fs::remove_dir_all(&staging);
            return Err(err);
        }

        let mut unlisted = Vec::new();
        for (i, path) in UNLISTED_FILES.iter().enumerate() {
            match fetch(path)? {
                Some(data) => {
                    report(data.len());
                    unlisted.push((*path, data));
                }
                None if i > 0 => report(0),
                None => {
                    return Err(Error::Download(format!(
                        "GitHub has no {path} for PoB2 {tag}"
                    )));
                }
            }
        }
        finish(parent, staging, &manifest, &manifest_text, &unlisted)
    }

    /// Installs from a PoB2 source archive already on disk (the zip GitHub
    /// serves for a release tag), with the same verification as `download`.
    pub fn from_archive(archive_path: &Path, parent: &Path) -> Result<Self> {
        let file = fs::File::open(archive_path).map_err(|e| Error::io("open", archive_path, e))?;
        let mut zip =
            zip::ZipArchive::new(file).map_err(|e| Error::Download(format!("bad archive: {e}")))?;

        // GitHub archives put everything under one top-level directory.
        let prefix = zip
            .file_names()
            .find_map(|n| {
                n.strip_suffix("manifest.xml")
                    .filter(|p| p.matches('/').count() == 1)
            })
            .map(str::to_owned)
            .ok_or_else(|| Error::Download("archive has no top-level manifest.xml".into()))?;

        let manifest_text = read_zip_entry(&mut zip, &format!("{prefix}manifest.xml"))?;
        let manifest_text = String::from_utf8(manifest_text)
            .map_err(|_| Error::Corrupt("manifest.xml is not UTF-8".into()))?;
        let manifest = parse_manifest(&manifest_text)?;

        let staging = fresh_staging(parent, &manifest.version)?;
        for file in &manifest.files {
            let bytes = read_zip_entry(&mut zip, &format!("{prefix}{}", file.path))?;
            check_hash(&file.path, &bytes, &file.sha1)?;
            write_file(&staging.join(&file.path), &bytes)?;
        }
        let mut unlisted = Vec::new();
        for (i, path) in UNLISTED_FILES.iter().enumerate() {
            let name = format!("{prefix}{path}");
            if i > 0 && zip.by_name(&name).is_err() {
                continue;
            }
            unlisted.push((*path, read_zip_entry(&mut zip, &name)?));
        }
        finish(parent, staging, &manifest, &manifest_text, &unlisted)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn version(&self) -> &PobVersion {
        &self.version
    }

    /// Directory PoB's code runs from.
    pub fn src_dir(&self) -> PathBuf {
        self.root.join("src")
    }

    /// Directory of the pure-Lua libraries PoB `require`s.
    pub fn lua_lib_dir(&self) -> PathBuf {
        self.root.join("runtime/lua")
    }
}

/// `path` with every byte outside RFC 3986's unreserved set (and `/`)
/// percent-encoded, e.g. spaces in PoB's file names.
fn url_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.bytes() {
        if byte.is_ascii_alphanumeric() || b"-._~/".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// How many files `download` fetches at once.
const PARALLEL_DOWNLOADS: usize = 8;

/// Where a download is: files fetched and verified so far, and their bytes.
#[derive(Debug, Clone, Copy, serde::Serialize)]
pub struct InstallProgress {
    pub done: usize,
    pub total: usize,
    pub bytes: u64,
}

/// Removes `.download-*.zip` files that earlier versions of the installer
/// downloaded (the whole PoB repository, ~390 MB) and may have left behind.
fn remove_stale_archives(parent: &Path) {
    let Ok(entries) = fs::read_dir(parent) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.starts_with(".download-") && name.ends_with(".zip") {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// An empty staging directory for `version` next to the installs.
fn fresh_staging(parent: &Path, version: &PobVersion) -> Result<PathBuf> {
    fs::create_dir_all(parent).map_err(|e| Error::io("create", parent, e))?;
    let staging = parent.join(format!(".staging-{version}"));
    if staging.exists() {
        fs::remove_dir_all(&staging).map_err(|e| Error::io("remove", &staging, e))?;
    }
    Ok(staging)
}

/// Writes the manifest and the unlisted engine files (recording their SHA-1)
/// into `staging`, then moves it into place as `<parent>/<version>`.
fn finish(
    parent: &Path,
    staging: PathBuf,
    manifest: &Manifest,
    manifest_text: &str,
    unlisted: &[(&str, Vec<u8>)],
) -> Result<Install> {
    write_file(&staging.join("manifest.xml"), manifest_text.as_bytes())?;
    let mut record = String::new();
    for (path, bytes) in unlisted {
        record.push_str(&format!("{}  {path}\n", pob_sha1(bytes)));
        write_file(&staging.join(path), bytes)?;
    }
    write_file(&staging.join(UNLISTED_RECORD), record.as_bytes())?;

    let root = parent.join(manifest.version.as_str());
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|e| Error::io("remove", &root, e))?;
    }
    fs::rename(&staging, &root).map_err(|e| Error::io("move into place", &root, e))?;
    Install::open(root)
}

fn parse_manifest(text: &str) -> Result<Manifest> {
    let doc = roxmltree::Document::parse(text)
        .map_err(|e| Error::Corrupt(format!("manifest.xml: {e}")))?;
    let root = doc.root_element();
    let version = root
        .children()
        .find(|n| n.has_tag_name("Version"))
        .and_then(|n| n.attribute("number"))
        .ok_or_else(|| Error::Corrupt("manifest.xml has no version".into()))?;

    let mut files = Vec::new();
    for node in root.children().filter(|n| n.has_tag_name("File")) {
        let (Some(name), Some(part), Some(sha1)) = (
            node.attribute("name"),
            node.attribute("part"),
            node.attribute("sha1"),
        ) else {
            return Err(Error::Corrupt(
                "manifest.xml has an incomplete <File>".into(),
            ));
        };
        // Platform binaries (runtime="win32") belong to the GUI app, not the engine.
        if node.attribute("runtime").is_some() || is_skipped(name) {
            continue;
        }
        let path = match part {
            "program" | "tree" => format!("src/{name}"),
            // The engine only uses runtime/lua; the rest is GUI runtime.
            "runtime" if name.starts_with("lua/") => format!("runtime/{name}"),
            _ => continue,
        };
        if path.split('/').any(|c| c == ".." || c.is_empty()) {
            return Err(Error::Corrupt(format!("unsafe path in manifest: {name}")));
        }
        files.push(ManifestFile {
            path,
            sha1: sha1.to_owned(),
        });
    }
    Ok(Manifest {
        version: PobVersion(version.to_owned()),
        files,
    })
}

fn parse_unlisted_record(text: &str) -> Result<Vec<ManifestFile>> {
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let (sha1, path) = line
                .split_once("  ")
                .ok_or_else(|| Error::Corrupt(format!("{UNLISTED_RECORD}: bad line {line:?}")))?;
            if !UNLISTED_FILES.contains(&path) {
                return Err(Error::Corrupt(format!(
                    "{UNLISTED_RECORD}: unexpected path {path}"
                )));
            }
            Ok(ManifestFile {
                path: path.to_owned(),
                sha1: sha1.to_owned(),
            })
        })
        .collect()
}

fn is_skipped(name: &str) -> bool {
    Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            SKIPPED_EXTENSIONS
                .iter()
                .any(|s| s.eq_ignore_ascii_case(ext))
        })
}

/// SHA-1 the way PoB's `update_manifest.py` computes it.
fn pob_sha1(bytes: &[u8]) -> String {
    let mut hasher = sha1_smol::Sha1::new();
    if bytes.contains(&0) {
        hasher.update(bytes);
    } else {
        let mut normalized = Vec::with_capacity(bytes.len() + bytes.len() / 32);
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\r' => {
                    normalized.extend_from_slice(b"\r\n");
                    if bytes.get(i + 1) == Some(&b'\n') {
                        i += 1;
                    }
                }
                b'\n' => normalized.extend_from_slice(b"\r\n"),
                b => normalized.push(b),
            }
            i += 1;
        }
        hasher.update(&normalized);
    }
    hasher.digest().to_string()
}

fn check_hash(path: &str, bytes: &[u8], expected: &str) -> Result<()> {
    let actual = pob_sha1(bytes);
    if actual.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(Error::Corrupt(format!(
            "{path}: SHA-1 {actual}, manifest says {expected}"
        )))
    }
}

fn read_zip_entry(zip: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut entry = zip
        .by_name(name)
        .map_err(|_| Error::Corrupt(format!("archive is missing {name}")))?;
    let mut bytes = Vec::with_capacity(entry.size() as usize);
    entry
        .read_to_end(&mut bytes)
        .map_err(|e| Error::Corrupt(format!("{name}: {e}")))?;
    Ok(bytes)
}

fn read_file(path: &Path) -> Result<String> {
    fs::read_to_string(path).map_err(|e| Error::io("read", path, e))
}

fn write_file(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|e| Error::io("create", dir, e))?;
    }
    fs::write(path, bytes).map_err(|e| Error::io("write", path, e))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_text_with_crlf_like_pob() {
        let mut h = sha1_smol::Sha1::new();
        h.update(b"a\r\nb\r\n");
        let expected = h.digest().to_string();
        assert_eq!(pob_sha1(b"a\nb\n"), expected);
        assert_eq!(pob_sha1(b"a\rb\r"), expected);
        assert_eq!(pob_sha1(b"a\r\nb\n"), expected);
    }

    #[test]
    fn hashes_binary_verbatim() {
        let binary = b"a\n\0b";
        let mut h = sha1_smol::Sha1::new();
        h.update(binary);
        assert_eq!(pob_sha1(binary), h.digest().to_string());
    }

    #[test]
    fn manifest_keeps_code_and_data_only() {
        let manifest = parse_manifest(
            r#"<PoBVersion><Version number="0.23.1"/>
            <File name="help.txt" part="default" sha1="1"/>
            <File name="Modules/Calcs.lua" part="program" sha1="2"/>
            <File name="TreeData/0_5/tree.lua" part="tree" sha1="3"/>
            <File name="TreeData/0_5/skills.png" part="tree" sha1="4"/>
            <File name="lua/xml.lua" part="runtime" sha1="5"/>
            <File name="lua51.dll" part="runtime" runtime="win32" sha1="6"/>
            </PoBVersion>"#,
        )
        .unwrap();
        assert_eq!(manifest.version.as_str(), "0.23.1");
        assert_eq!(manifest.version.tag(), "v0.23.1");
        let paths: Vec<_> = manifest.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "src/Modules/Calcs.lua",
                "src/TreeData/0_5/tree.lua",
                "runtime/lua/xml.lua"
            ]
        );
    }

    #[test]
    fn unlisted_record_round_trips_and_rejects_strangers() {
        let files = parse_unlisted_record("abc  src/HeadlessWrapper.lua\n").unwrap();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "src/HeadlessWrapper.lua");
        assert_eq!(files[0].sha1, "abc");
        assert!(parse_unlisted_record("abc  src/Evil.lua\n").is_err());
        assert!(parse_unlisted_record("no-separator\n").is_err());
    }

    #[test]
    fn encodes_url_paths() {
        assert_eq!(url_path("src/Data/Bases/axe.lua"), "src/Data/Bases/axe.lua");
        assert_eq!(url_path("runtime/a b+c.lua"), "runtime/a%20b%2Bc.lua");
    }

    #[test]
    fn manifest_rejects_path_traversal() {
        let text = r#"<PoBVersion><Version number="1"/><File name="../evil.lua" part="program" sha1="0"/></PoBVersion>"#;
        assert!(matches!(parse_manifest(text), Err(Error::Corrupt(_))));
    }
}
