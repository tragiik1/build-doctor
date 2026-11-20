//! Read-only lookups for questions about a build: what it wears, sockets and
//! allocates, any number in PoB's output, and PoB's own data for unique
//! items and gems. Everything is PoB's data or PoB's result, as JSON.

use serde_json::Value;

use crate::{Engine, Error, Result};

impl Engine {
    /// The loaded build's items (by slot, with mod lines), skill groups
    /// (with gems) and allocated keystones, notables and ascendancy passives
    /// (with PoB's text for each).
    pub fn build_details(&self) -> Result<Value> {
        self.json("bd_build_details", ())
    }

    /// Numbers in PoB's output for the loaded build whose key contains
    /// `filter` (any case): `{ total, stats: [{ key, value }] }`, at most
    /// `limit` of them.
    pub fn output_keys(&self, filter: &str, limit: usize) -> Result<Value> {
        self.json("bd_output_keys", (filter, limit))
    }

    /// Unique items in PoB's data by name: the item's text for an exact name,
    /// otherwise the names containing `query`.
    pub fn find_unique(&self, query: &str) -> Result<Value> {
        self.json("bd_find_unique", query)
    }

    /// A gem from PoB's data by name: description, tags, requirements and
    /// what it does at `level` (its natural maximum when `None`), in the
    /// game's wording.
    pub fn gem_info(&self, query: &str, level: Option<u32>) -> Result<Value> {
        self.json("bd_gem_info", (query, level))
    }

    fn json<A: mlua::IntoLuaMulti>(&self, function: &str, args: A) -> Result<Value> {
        let text: String = self.call(function, args)?;
        serde_json::from_str(&text)
            .map_err(|e| Error::Pob(format!("{function} returned unreadable JSON: {e}")))
    }
}

#[cfg(test)]
mod tests {
    use crate::{Engine, Install};

    fn engine() -> Option<Engine> {
        let dir = std::env::var_os("DOCTOR_POB_DIR")?;
        Some(Engine::start(&Install::open(dir).ok()?).expect("engine starts"))
    }

    #[test]
    #[ignore = "needs a PoB install (DOCTOR_POB_DIR)"]
    fn finds_uniques_and_gems_in_pob_data() {
        let Some(engine) = engine() else { return };
        let unique = engine.find_unique("adonia's ego").unwrap();
        assert_eq!(unique["exact"], true);
        let text = unique["items"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("Adonia's Ego\nSiphoning Wand"), "{text}");

        let partial = engine.find_unique("ego").unwrap();
        assert_eq!(partial["exact"], false);
        assert!(!partial["items"].as_array().unwrap().is_empty());

        let gem = engine.gem_info("snap", Some(20)).unwrap();
        assert_eq!(gem["found"], true);
        assert_eq!(gem["name"], "Snap");
        assert_eq!(gem["level"], 20);
        let lines = gem["statSets"][0]["lines"].as_array().unwrap();
        assert!(!lines.is_empty(), "{gem}");

        let missing = engine.gem_info("snapp", None).unwrap();
        assert_eq!(missing["found"], false);
    }
}
