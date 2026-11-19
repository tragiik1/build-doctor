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

use std::fmt;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use crate::{Error, Result};

const REPOSITORY: &str = "PathOfBuildingCommunity/PathOfBuilding-PoE2";

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

    /// Re-hashes every kept file against the manifest.
    pub fn verify(&self) -> Result<()> {
        let manifest = parse_manifest(&read_file(&self.root.join("manifest.xml"))?)?;
        for file in &manifest.files {
            let path = self.root.join(&file.path);
            let bytes = fs::read(&path).map_err(|e| Error::io("read", &path, e))?;
            check_hash(&file.path, &bytes, &file.sha1)?;
        }
        Ok(())
    }

    /// Downloads PoB2 release `tag` from GitHub into `<parent>/<version>/`,
    /// verifying every file, and returns the install. An existing install of
    /// the same version is replaced only after the new one verifies.
    pub fn download(tag: &str, parent: &Path) -> Result<Self> {
        fs::create_dir_all(parent).map_err(|e| Error::io("create", parent, e))?;
        let archive_path = parent.join(format!(".download-{tag}.zip"));
        let url = format!("https://codeload.github.com/{REPOSITORY}/zip/refs/tags/{tag}");
        fetch_to_file(&url, &archive_path)?;
        let result = Self::extract(&archive_path, parent);
        let _ = fs::remove_file(&archive_path);
        result
    }

    fn extract(archive_path: &Path, parent: &Path) -> Result<Self> {
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

        let staging = parent.join(format!(".staging-{}", manifest.version));
        if staging.exists() {
            fs::remove_dir_all(&staging).map_err(|e| Error::io("remove", &staging, e))?;
        }
        for file in &manifest.files {
            let bytes = read_zip_entry(&mut zip, &format!("{prefix}{}", file.path))?;
            check_hash(&file.path, &bytes, &file.sha1)?;
            write_file(&staging.join(&file.path), &bytes)?;
        }
        write_file(&staging.join("manifest.xml"), manifest_text.as_bytes())?;

        let root = parent.join(manifest.version.as_str());
        if root.exists() {
            fs::remove_dir_all(&root).map_err(|e| Error::io("remove", &root, e))?;
        }
        fs::rename(&staging, &root).map_err(|e| Error::io("move into place", &root, e))?;
        Self::open(root)
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
            "runtime" => format!("runtime/{name}"),
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

fn fetch_to_file(url: &str, dest: &Path) -> Result<()> {
    let response = ureq::get(url)
        .header(
            "User-Agent",
            concat!("build-doctor/", env!("CARGO_PKG_VERSION")),
        )
        .call()
        .map_err(|e| Error::Download(format!("{url}: {e}")))?;
    let mut reader = response.into_body().into_reader();
    let mut file = fs::File::create(dest).map_err(|e| Error::io("create", dest, e))?;
    io::copy(&mut reader, &mut file).map_err(|e| Error::Download(format!("{url}: {e}")))?;
    Ok(())
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
    fn manifest_rejects_path_traversal() {
        let text = r#"<PoBVersion><Version number="1"/><File name="../evil.lua" part="program" sha1="0"/></PoBVersion>"#;
        assert!(matches!(parse_manifest(text), Err(Error::Corrupt(_))));
    }
}
