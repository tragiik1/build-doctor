//! Turns whatever the user pasted into build XML.

use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::{Error, Result};

/// Build-sharing sites PoB2 itself imports from, mapped to their raw-code
/// endpoints (same mapping as PoB2's `Modules/BuildSiteTools.lua`).
const SITES: &[(&str, &str)] = &[
    ("https://pobb.in/", "https://pobb.in/pob/{id}"),
    (
        "https://poe.ninja/poe2/pob/",
        "https://poe.ninja/poe2/pob/raw/{id}",
    ),
    (
        "https://maxroll.gg/poe2/pob/",
        "https://maxroll.gg/poe2/api/pob/{id}",
    ),
    ("https://poe2db.tw/pob/", "https://poe2db.tw/pob/{id}/raw"),
];

/// Turns what the user pasted (a PoB code, build XML, a path to either, or a
/// pobb.in / poe.ninja / maxroll / poe2db link) into build XML.
pub fn read_build(input: &str) -> Result<String> {
    let input = input.trim();
    if let Some(url) = raw_url(input) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(concat!("build-doctor/", env!("CARGO_PKG_VERSION")))
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        let code = agent
            .get(&url)
            .call()
            .map_err(|e| Error::Download(format!("{url}: {e}")))?
            .into_body()
            .read_to_string()
            .map_err(|e| Error::Download(format!("{url}: {e}")))?;
        return crate::code::decode(&code);
    }
    if input.starts_with("http://") || input.starts_with("https://") {
        return Err(Error::Code(format!(
            "unsupported link; supported sites: {}",
            site_list()
        )));
    }

    let path = Path::new(input);
    let text = if path.is_file() {
        fs::read_to_string(path).map_err(|e| Error::io("read", path, e))?
    } else {
        input.to_owned()
    };
    let trimmed = text.trim_start_matches('\u{feff}').trim();
    if trimmed.starts_with('<') {
        Ok(trimmed.to_owned())
    } else {
        crate::code::decode(trimmed)
    }
}

fn raw_url(input: &str) -> Option<String> {
    SITES.iter().find_map(|(prefix, raw)| {
        let id = input.strip_prefix(prefix)?.trim_end_matches('/');
        let valid = !id.is_empty()
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
        valid.then(|| raw.replace("{id}", id))
    })
}

fn site_list() -> String {
    SITES
        .iter()
        .map(|(prefix, _)| prefix.trim_end_matches('/'))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_share_links_to_raw_endpoints() {
        assert_eq!(
            raw_url("https://pobb.in/MEY40cJToiDI").as_deref(),
            Some("https://pobb.in/pob/MEY40cJToiDI")
        );
        assert_eq!(
            raw_url("https://pobb.in/_hu1-w85QHxP/").as_deref(),
            Some("https://pobb.in/pob/_hu1-w85QHxP")
        );
        assert_eq!(
            raw_url("https://poe.ninja/poe2/pob/abc").as_deref(),
            Some("https://poe.ninja/poe2/pob/raw/abc")
        );
        assert_eq!(raw_url("https://pobb.in/a/../b"), None);
        assert_eq!(raw_url("https://example.com/x"), None);
    }

    #[test]
    fn accepts_xml_and_codes() {
        let xml = "<PathOfBuilding2><Build/></PathOfBuilding2>";
        assert_eq!(read_build(xml).unwrap(), xml);
        assert_eq!(read_build(&crate::code::encode(xml)).unwrap(), xml);
    }
}
