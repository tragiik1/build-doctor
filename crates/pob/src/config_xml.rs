//! Editing a build's configuration in its XML.

use crate::{Error, Result};

/// Returns `xml` with the active config set's `<Input>` entries for `vars`
/// removed: the build as if those options were unticked or cleared in PoB.
/// Everything else is kept byte for byte.
pub fn without_inputs(xml: &str, vars: &[&str]) -> Result<String> {
    let doc = roxmltree::Document::parse(xml).map_err(|e| Error::Xml(e.to_string()))?;
    let Some(config) = doc.descendants().find(|n| n.has_tag_name("Config")) else {
        return Ok(xml.to_owned());
    };
    let active = config.attribute("activeConfigSet").unwrap_or("1");
    let sets: Vec<_> = config
        .children()
        .filter(|n| n.has_tag_name("ConfigSet"))
        .collect();
    // Older builds keep <Input> directly under <Config>.
    let set = sets
        .iter()
        .find(|n| n.attribute("id") == Some(active))
        .copied()
        .unwrap_or(config);

    let mut ranges: Vec<_> = set
        .children()
        .filter(|n| {
            n.has_tag_name("Input") && n.attribute("name").is_some_and(|name| vars.contains(&name))
        })
        .map(|n| n.range())
        .collect();
    ranges.sort_by_key(|r| std::cmp::Reverse(r.start));
    let mut out = xml.to_owned();
    for range in ranges {
        // Take the element's own line with it, so the file stays tidy.
        let start = out[..range.start]
            .rfind('\n')
            .map_or(range.start, |i| i + 1);
        let start = if out[start..range.start].trim().is_empty() {
            start
        } else {
            range.start
        };
        let end = if out[range.end..].starts_with('\n') {
            range.end + 1
        } else {
            range.end
        };
        out.replace_range(start..end, "");
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = "<PathOfBuilding2>\n\t<Config activeConfigSet=\"2\">\n\t\t<ConfigSet id=\"1\">\n\t\t\t<Input boolean=\"true\" name=\"conditionEnemyShocked\"/>\n\t\t</ConfigSet>\n\t\t<ConfigSet id=\"2\">\n\t\t\t<Input boolean=\"true\" name=\"conditionEnemyShocked\"/>\n\t\t\t<Input number=\"3\" name=\"multiplierPowerCharge\"/>\n\t\t</ConfigSet>\n\t</Config>\n</PathOfBuilding2>";

    #[test]
    fn removes_only_named_inputs_of_the_active_set() {
        let out = without_inputs(XML, &["conditionEnemyShocked"]).unwrap();
        assert_eq!(
            out.matches("conditionEnemyShocked").count(),
            1,
            "set 1 keeps its copy"
        );
        assert!(out.contains("multiplierPowerCharge"));
        assert!(roxmltree::Document::parse(&out).is_ok());
    }

    #[test]
    fn removing_nothing_changes_nothing() {
        assert_eq!(without_inputs(XML, &[]).unwrap(), XML);
        assert_eq!(without_inputs(XML, &["notThere"]).unwrap(), XML);
    }
}
