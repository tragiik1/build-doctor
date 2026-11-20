//! What the coach can look up or test. Every tool runs Path of Building on
//! the player's build or reads PoB's own game data; none computes a game
//! number itself. Web search and page fetches run on Anthropic's side,
//! limited to the sites below.

use std::collections::HashMap;

use pob::Pool;
use serde::Deserialize;
use serde_json::{Value, json};

/// Sites the coach may search and read: the community wiki, the game-data
/// database, the official site (patch notes, forum) and two guide sites.
pub const SITES: &[&str] = &[
    "poe2wiki.net",
    "poe2db.tw",
    "pathofexile.com",
    "maxroll.gg",
    "mobalytics.gg",
];

const MAX_LINES: usize = 12;
const MAX_STATS: usize = 24;
const SEARCH_RESULTS: usize = 60;
/// Stats per goal kept from a slot's measured values.
const SLOT_STATS: usize = 15;

/// The build the tools work on.
#[derive(Debug, Clone, Copy)]
pub struct Bench<'a> {
    pub pool: &'a Pool,
    pub xml: &'a str,
}

/// A tool's answer for the model.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub content: String,
    pub is_error: bool,
}

impl Outcome {
    fn ok(value: &Value) -> Self {
        Self {
            content: value.to_string(),
            is_error: false,
        }
    }

    fn error(message: impl Into<String>) -> Self {
        Self {
            content: message.into(),
            is_error: true,
        }
    }
}

/// The tools the app runs, for the request's `tools` (streamed input; see
/// the API module).
pub fn definitions() -> Vec<Value> {
    let tools = [
        json!({
            "name": "what_if_modifiers",
            "description": "Recalculate the player's build in Path of Building with extra modifier lines added, exactly as if typed into PoB's Custom Modifiers box, and return every shown stat that changed (before, after, change, percent) plus any new PoB warnings. Use this whenever the answer depends on what a change would do: more resistance, a different notable's stats, a gear stat, more levels on gems, a buff. Write each line the way it appears on an item or passive in game, for example \"+30% to Fire Resistance\", \"25% increased Spell Damage\", \"+1 to Level of all Spell Skills\", \"+50 to maximum Life\". Several lines in one call are applied together; to compare options, make one call per option. PoB rejects lines it cannot read; reword them as game text. The build is restored afterwards.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "lines": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": MAX_LINES, "description": "Modifier lines, as written in game."},
                    "stats": {"type": "array", "items": {"type": "string"}, "maxItems": MAX_STATS, "description": "Extra PoB output stats to report (keys from get_stats), besides the shown ones."}
                },
                "required": ["lines"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "get_stats",
            "description": "Read numbers from Path of Building's calculation for the player's build. Pass `keys` for exact PoB output keys (for example \"FireResist\", \"MissingFireResist\", \"TotalEHP\", \"ManaUnreserved\", \"BlockChance\", \"CritMultiplier\"), or `search` to list every output whose key contains a word (for example \"Resist\", \"Ailment\", \"Leech\"). Use it for any number about the build not in the build data you were given.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "keys": {"type": "array", "items": {"type": "string"}, "maxItems": 40},
                    "search": {"type": "string", "description": "Part of a key, any case."}
                },
                "additionalProperties": false
            }
        }),
        json!({
            "name": "equip_item",
            "description": "Put an item on the player's build in Path of Building, in each slot it fits, and return what changes in every slot (shown stats before and after, new warnings) and any item lines PoB cannot read. Pass the item's full text as the game copies it with Ctrl+C, or a unique's text from find_unique (PoB uses the middle of each roll range).",
            "input_schema": {
                "type": "object",
                "properties": {"item_text": {"type": "string"}},
                "required": ["item_text"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "slot_values",
            "description": "Measure, for one equipment slot, what every stat that can roll there is worth to the player's build, each at a middle roll on a blank rare of the worn base, in percent of the current damage, effective health and smallest max hit. Also says what the worn item adds over a blank one. Use it for 'what should I look for on my gloves' or 'what is my weakest slot'. Slow (it measures every stat), so ask for one slot at a time. Slot names are the `slot` fields in the build data (\"Helmet\", \"Ring 1\", \"Weapon 1\", ...).",
            "input_schema": {
                "type": "object",
                "properties": {"slot": {"type": "string"}},
                "required": ["slot"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "find_unique",
            "description": "Look up a unique item in Path of Building's game data by name: its base, requirements and every mod line with roll ranges and variants. An exact name returns the item; a partial name lists matches. Always use this before describing what a unique does; never describe one from memory.",
            "input_schema": {
                "type": "object",
                "properties": {"name": {"type": "string"}},
                "required": ["name"],
                "additionalProperties": false
            }
        }),
        json!({
            "name": "gem_info",
            "description": "Look up a skill or support gem in Path of Building's game data by name: description, tags, attribute requirements, cost, cooldown and reservation, and what it does at a given level in the game's own wording (damage, durations, radius), plus its quality bonus. Use this before describing what a gem does; never from memory. Name matching is exact (any case); close names are listed when none matches.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "name": {"type": "string"},
                    "level": {"type": "integer", "minimum": 1, "maximum": 40, "description": "Gem level; its natural maximum when left out."}
                },
                "required": ["name"],
                "additionalProperties": false
            }
        }),
    ];
    tools
        .into_iter()
        .map(|mut tool| {
            tool["eager_input_streaming"] = Value::Bool(true);
            tool
        })
        .collect()
}

/// Web search and page fetch, run by Anthropic, on the allowed sites only.
pub fn server_tools() -> Vec<Value> {
    vec![
        json!({
            "type": "web_search_20260209",
            "name": "web_search",
            "max_uses": 5,
            "allowed_domains": SITES,
        }),
        json!({
            "type": "web_fetch_20260209",
            "name": "web_fetch",
            "max_uses": 4,
            "allowed_domains": SITES,
            "citations": {"enabled": true},
            "max_content_tokens": 12000,
        }),
    ]
}

/// A one-line description of a tool call, for the "how this was checked"
/// list.
pub fn describe(name: &str, input: &Value) -> String {
    let text = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or("").trim();
    let list = |key: &str| {
        input
            .get(key)
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .collect::<Vec<_>>()
                    .join("; ")
            })
            .unwrap_or_default()
    };
    match name {
        "what_if_modifiers" => format!("Tested in PoB: {}", list("lines")),
        "get_stats" if !text("search").is_empty() => {
            format!("Read PoB stats matching \"{}\"", text("search"))
        }
        "get_stats" => format!("Read PoB stats: {}", list("keys")),
        "equip_item" => {
            let first = text("item_text")
                .lines()
                .find(|l| !l.trim().is_empty() && !l.contains(':'))
                .unwrap_or("an item");
            format!("Equipped in PoB: {first}")
        }
        "slot_values" => format!("Measured every stat for {}", or(text("slot"), "a slot")),
        "find_unique" => format!("Looked up {} in PoB's data", or(text("name"), "a unique")),
        "gem_info" => format!("Looked up {} in PoB's data", or(text("name"), "a gem")),
        "web_search" => format!("Searched: {}", text("query")),
        "web_fetch" => format!("Read {}", text("url")),
        other => format!("Used {other}"),
    }
}

fn or<'a>(text: &'a str, fallback: &'a str) -> &'a str {
    if text.is_empty() { fallback } else { text }
}

/// Measured slot values, kept for the conversation (they are slow).
pub type SlotCache = HashMap<String, Value>;

/// Runs one of [`definitions`]'s tools.
pub fn run(bench: Bench<'_>, cache: &mut SlotCache, name: &str, input: &Value) -> Outcome {
    let result = match name {
        "what_if_modifiers" => parse(input).and_then(|i| what_if(bench, i)),
        "get_stats" => parse(input).and_then(|i| get_stats(bench, i)),
        "equip_item" => parse(input).and_then(|i| equip_item(bench, i)),
        "slot_values" => parse(input).and_then(|i| slot_values(bench, cache, i)),
        "find_unique" => {
            parse(input).and_then(|i: Name| lookup(bench, move |e| e.find_unique(&i.name)))
        }
        "gem_info" => {
            parse(input).and_then(|i: Gem| lookup(bench, move |e| e.gem_info(&i.name, i.level)))
        }
        other => Err(format!("There is no tool named {other}.")),
    };
    match result {
        Ok(value) => Outcome::ok(&value),
        Err(message) => Outcome::error(message),
    }
}

fn parse<T: for<'de> Deserialize<'de>>(input: &Value) -> Result<T, String> {
    serde_json::from_value(input.clone()).map_err(|e| format!("Invalid input: {e}"))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WhatIf {
    lines: Vec<String>,
    #[serde(default)]
    stats: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Stats {
    #[serde(default)]
    keys: Vec<String>,
    #[serde(default)]
    search: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    item_text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Slot {
    slot: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Name {
    name: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Gem {
    name: String,
    #[serde(default)]
    level: Option<u32>,
}

/// Runs `job` on one engine with the build loaded.
pub(crate) fn on_engine<T: Send + 'static>(
    bench: Bench<'_>,
    job: impl Fn(&pob::Engine) -> pob::Result<T> + Send + Sync + 'static,
) -> Result<T, String> {
    bench
        .pool
        .map(bench.xml, None, vec![()], move |engine, ()| job(engine))
        .map_err(|e| e.to_string())?
        .pop()
        .ok_or_else(|| "PoB returned nothing.".to_owned())
}

fn lookup(
    bench: Bench<'_>,
    job: impl Fn(&pob::Engine) -> pob::Result<Value> + Send + Sync + 'static,
) -> Result<Value, String> {
    on_engine(bench, job)
}

fn what_if(bench: Bench<'_>, input: WhatIf) -> Result<Value, String> {
    let lines: Vec<String> = input
        .lines
        .iter()
        .map(|l| l.trim().to_owned())
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() || lines.len() > MAX_LINES {
        return Err(format!("Give between 1 and {MAX_LINES} modifier lines."));
    }
    let mut labels: Vec<(String, String)> = analysis::overview::shown_stats()
        .map(|(key, group, label)| (key.to_owned(), format!("{group}: {label}")))
        .collect();
    for key in input.stats.iter().take(MAX_STATS) {
        if !labels.iter().any(|(k, _)| k == key) {
            labels.push((key.clone(), key.clone()));
        }
    }
    let keys: Vec<String> = labels.iter().map(|(k, _)| k.clone()).collect();
    let tested = lines.clone();
    let (before, after, warned_before, warned_after) = on_engine(bench, move |engine| {
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        let before = engine.outputs(&keys)?;
        let warned_before = engine.warnings()?;
        let (_, after, warned_after) = engine.with_mods_warned(&lines, &keys)?;
        Ok((before, after, warned_before, warned_after))
    })
    .map_err(|e| {
        if e.contains("cannot read the modifier") {
            format!("{e}. Write it exactly as the line reads on an item or passive in game.")
        } else {
            e
        }
    })?;
    let mut changed = Vec::new();
    let mut unchanged = Vec::new();
    for (key, label) in &labels {
        match (before.get(key), after.get(key)) {
            (Some(b), Some(a)) if (a - b).abs() > 1e-9 => changed.push(json!({
                "stat": key,
                "label": label,
                "before": b,
                "after": a,
                "change": a - b,
                "percent": analysis::pct(*b, *a),
            })),
            (Some(_), Some(_)) => unchanged.push(label.clone()),
            _ => {}
        }
    }
    let new_warnings: Vec<&String> = warned_after
        .iter()
        .filter(|w| !warned_before.contains(w))
        .collect();
    Ok(json!({
        "tested": tested,
        "changed": changed,
        "unchanged": unchanged,
        "newWarnings": new_warnings,
    }))
}

fn get_stats(bench: Bench<'_>, input: Stats) -> Result<Value, String> {
    let search = input.search.filter(|s| !s.trim().is_empty());
    if input.keys.is_empty() && search.is_none() {
        return Err("Give `keys` or `search`.".into());
    }
    let keys = input.keys;
    on_engine(bench, move |engine| {
        let mut out = json!({});
        if !keys.is_empty() {
            let wanted: Vec<&str> = keys.iter().map(String::as_str).collect();
            let values = engine.outputs(&wanted)?;
            let missing: Vec<&str> = wanted
                .iter()
                .copied()
                .filter(|k| !values.contains_key(*k))
                .collect();
            out["values"] = json!(values);
            if !missing.is_empty() {
                out["notInPoBOutput"] = json!(missing);
            }
        }
        if let Some(search) = &search {
            out["search"] = engine.output_keys(search.trim(), SEARCH_RESULTS)?;
        }
        Ok(out)
    })
}

fn equip_item(bench: Bench<'_>, input: Item) -> Result<Value, String> {
    let check =
        analysis::item_check(bench.pool, bench.xml, &input.item_text).map_err(|e| e.to_string())?;
    serde_json::to_value(check).map_err(|e| e.to_string())
}

fn slot_values(bench: Bench<'_>, cache: &mut SlotCache, input: Slot) -> Result<Value, String> {
    if let Some(found) = cache.get(&input.slot) {
        return Ok(found.clone());
    }
    let values =
        analysis::slot_values(bench.pool, bench.xml, &input.slot).map_err(|e| e.to_string())?;
    let top = |score: fn(&analysis::gear::StatWorth) -> f64| -> Vec<Value> {
        let mut stats: Vec<&analysis::gear::StatWorth> =
            values.stats.iter().filter(|s| score(s) > 0.0).collect();
        stats.sort_by(|a, b| score(b).total_cmp(&score(a)));
        stats
            .into_iter()
            .take(SLOT_STATS)
            .map(|s| {
                json!({
                    "line": s.line,
                    "implicit": s.implicit,
                    "damagePercent": s.share.damage,
                    "ehpPercent": s.share.ehp,
                    "minHitPercent": s.share.min_hit,
                })
            })
            .collect()
    };
    let out = json!({
        "slot": values.slot,
        "usesFullDps": values.uses_full_dps,
        "wornItemOverBlank": values.worn_item,
        "measuredStats": values.measured,
        "bestForDamage": top(|s| s.share.damage),
        "bestForEffectiveHealth": top(|s| s.share.ehp),
        "bestForSmallestMaxHit": top(|s| s.share.min_hit),
        "note": "Each stat alone at a middle roll on a blank rare of the worn base; percent of the build's current numbers.",
    });
    cache.insert(input.slot, out.clone());
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tools_are_well_formed() {
        let tools = definitions();
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        assert_eq!(
            names,
            [
                "what_if_modifiers",
                "get_stats",
                "equip_item",
                "slot_values",
                "find_unique",
                "gem_info"
            ]
        );
        for tool in &tools {
            assert_eq!(tool["eager_input_streaming"], true);
            assert_eq!(tool["input_schema"]["type"], "object");
            assert!(tool["description"].as_str().unwrap().len() > 80);
        }
        for tool in server_tools() {
            assert_eq!(
                tool["allowed_domains"].as_array().unwrap().len(),
                SITES.len()
            );
        }
    }

    #[test]
    fn describes_calls_plainly() {
        assert_eq!(
            describe(
                "what_if_modifiers",
                &json!({"lines": ["+30% to Fire Resistance", "+50 to maximum Life"]})
            ),
            "Tested in PoB: +30% to Fire Resistance; +50 to maximum Life"
        );
        assert_eq!(
            describe(
                "equip_item",
                &json!({"item_text": "Item Class: Rings\nRarity: Rare\nGale Loop\nRuby Ring\n..."})
            ),
            "Equipped in PoB: Gale Loop"
        );
        assert_eq!(
            describe("web_search", &json!({"query": "poe2 armour formula"})),
            "Searched: poe2 armour formula"
        );
        assert_eq!(
            describe("find_unique", &json!({})),
            "Looked up a unique in PoB's data"
        );
    }

    #[test]
    fn bad_input_is_an_error_for_the_model() {
        let input = json!({"lines": "not a list"});
        let parsed: Result<WhatIf, String> = parse(&input);
        assert!(parsed.err().unwrap().starts_with("Invalid input"));
        let unknown: Result<Gem, String> = parse(&json!({"name": "Snap", "lvl": 3}));
        assert!(unknown.is_err());
    }
}
