//! What to look for on gear: for a worn item, what each stat that can roll
//! in its slot is worth to the build, and a trade search ranked by it.
//!
//! Measured the way PoB's own trade search measures it: the worn item is
//! replaced by a blank rare of the same base, which then gets one stat at a
//! time at the middle of its roll range (PoB's roll data), recalculated with
//! PoB's calculator. Values are shown as a share of the build's current
//! numbers. Stats measured alone can be worth more or less together, which
//! is why a candidate from the search is then checked exactly with
//! [`crate::item_check`].

use pob::{GearSlot, Pool, Summary};
use serde::{Deserialize, Serialize};

use crate::{BuildInfo, Result, damage};

/// Stats in the weighted search. The trade site allows 35 filters in all;
/// past the top 20 the weights are too small to change the order, and every
/// stat makes the link longer.
const MAX_STATS: usize = 20;
/// Smallest share of a number, in percent, worth listing.
const MIN_SHARE: f64 = 0.05;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Gear {
    pub build: BuildInfo,
    pub slots: Vec<GearSlot>,
}

/// The worn items a trade search can replace.
pub fn gear(pool: &Pool, xml: &str) -> Result<Gear> {
    let mut found = pool.map(xml, None, vec![()], |engine, ()| {
        Ok((engine.identity()?, engine.gear_slots()?))
    })?;
    let (id, slots) = found.pop().expect("one item in, one out");
    Ok(Gear {
        build: BuildInfo::new(&id, &pool.install().version().to_string()),
        slots,
    })
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotValues {
    pub slot: GearSlot,
    pub uses_full_dps: bool,
    /// What the worn item adds over a blank rare of the same base, in
    /// percent of the current numbers.
    pub worn_item: Share,
    /// Stats worth something to the build, best for damage and survival
    /// together first.
    pub stats: Vec<StatWorth>,
    /// Trade stats measured.
    pub measured: usize,
}

/// Changes in percent of the build's current numbers.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Share {
    pub damage: f64,
    pub ehp: f64,
    pub min_hit: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatWorth {
    pub trade_id: String,
    /// The mod line PoB evaluated, at the mid roll.
    pub line: String,
    pub value: f64,
    /// An implicit (the base's own line) rather than a rolled mod.
    pub implicit: bool,
    /// What the stat adds to a blank item in the slot, in percent of the
    /// build's current numbers.
    pub share: Share,
}

/// Measures every trade stat that can roll in `slot`, split across the
/// pool's engines.
pub fn slot_values(pool: &Pool, xml: &str, slot: &str) -> Result<SlotValues> {
    let gear = gear(pool, xml)?;
    let worn = gear
        .slots
        .into_iter()
        .find(|s| s.slot == slot)
        .ok_or_else(|| {
            crate::Error::Analysis(format!(
                "No item a trade search can replace is worn in {slot}"
            ))
        })?;

    let parts = pool.size();
    let name = slot.to_owned();
    let results = pool.map(xml, None, (0..parts).collect(), move |engine, part| {
        engine.stat_values(&name, part, parts)
    })?;
    let first = results.first().expect("at least one part");
    let (current, blank) = (first.current, first.blank);
    let uses_full_dps = current.full_dps > 0.0;
    let share = |with: &Summary, from: &Summary| Share {
        damage: pct_of(
            damage(uses_full_dps, &current),
            damage(uses_full_dps, from),
            damage(uses_full_dps, with),
        ),
        ehp: pct_of(current.ehp, from.ehp, with.ehp),
        min_hit: pct_of(current.min_hit, from.min_hit, with.min_hit),
    };

    let measured = results.iter().map(|r| r.stats.len()).sum();
    let mut stats: Vec<StatWorth> = results
        .into_iter()
        .flat_map(|r| r.stats)
        .map(|s| StatWorth {
            share: share(&s.with_stat, &blank),
            implicit: s.trade_id.starts_with("implicit."),
            trade_id: s.trade_id,
            line: s.line,
            value: s.value,
        })
        .filter(|s| score(Goal::Balanced, &s.share) >= MIN_SHARE)
        .collect();
    stats.sort_by(|a, b| {
        score(Goal::Balanced, &b.share)
            .total_cmp(&score(Goal::Balanced, &a.share))
            .then_with(|| a.trade_id.cmp(&b.trade_id))
    });

    Ok(SlotValues {
        worn_item: share(&current, &blank),
        slot: worn,
        uses_full_dps,
        stats,
        measured,
    })
}

/// `(after - before)` in percent of `base`; 0 when any value is not finite
/// (an immunity) or `base` is 0.
fn pct_of(base: f64, before: f64, after: f64) -> f64 {
    if !(base.is_finite() && before.is_finite() && after.is_finite()) || base == 0.0 {
        return 0.0;
    }
    (after - before) / base * 100.0
}

/// What a trade search ranks by.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Goal {
    Damage,
    /// Total EHP and smallest max hit.
    Survival,
    /// Damage, Total EHP and smallest max hit.
    Balanced,
}

/// Sum of the gains a goal counts (losses count against it), in percent.
pub fn score(goal: Goal, s: &Share) -> f64 {
    match goal {
        Goal::Damage => s.damage,
        Goal::Survival => s.ehp + s.min_hit,
        Goal::Balanced => s.damage + s.ehp + s.min_hit,
    }
}

/// What the trade search looks for.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TradeOptions {
    pub league: String,
    pub goal: Goal,
    /// Highest price, in `currency`.
    pub max_price: Option<f64>,
    /// The trade site's currency id: "exalted", "divine" or "chaos".
    pub currency: Option<String>,
    /// Search every base of the category, not only the worn item's. Only
    /// matters for weapons and armour (see [`SlotValues::same_base_only`]).
    #[serde(default)]
    pub any_base: bool,
}

const CURRENCIES: &[&str] = &["exalted", "divine", "chaos"];

impl SlotValues {
    /// Weapons and armour: a base's own damage or defences are not in the
    /// weights (they are not mods), so other bases would rank wrongly, and
    /// the search keeps to the worn item's base unless asked otherwise.
    /// Jewellery bases differ only in their implicit, which is weighted.
    pub fn same_base_only(&self) -> bool {
        let c = &self.slot.trade_category;
        c.starts_with("weapon.") || c.starts_with("armour.")
    }
}

/// A trade site link (no request is made) searching the slot's category for
/// non-unique items, sorted by a weighted sum of the stats worth most for
/// the goal. Each weight is the stat's share per point of its value, so an
/// item's sum estimates its total gain in percent, stats measured alone.
pub fn trade_url(values: &SlotValues, options: &TradeOptions) -> Result<String> {
    let league = options.league.trim();
    if league.is_empty() {
        return Err(crate::Error::Analysis("Type your league first".into()));
    }
    let mut ranked: Vec<&StatWorth> = values
        .stats
        .iter()
        .filter(|s| s.value > 0.0 && score(options.goal, &s.share) >= MIN_SHARE)
        .collect();
    ranked.sort_by(|a, b| score(options.goal, &b.share).total_cmp(&score(options.goal, &a.share)));
    ranked.truncate(MAX_STATS);
    if ranked.is_empty() {
        return Err(crate::Error::Analysis(
            "No stat in this slot adds to that goal, so there is nothing to search for".into(),
        ));
    }

    let weights: Vec<serde_json::Value> = ranked
        .iter()
        .map(|s| {
            let weight = round_significant(score(options.goal, &s.share) / s.value, 3);
            serde_json::json!({ "id": s.trade_id, "value": { "weight": weight } })
        })
        .collect();
    let mut filters = serde_json::json!({
        "type_filters": { "filters": {
            "category": { "option": values.slot.trade_category },
            "rarity": { "option": "nonunique" },
        }},
        "misc_filters": { "disabled": false, "filters": { "mirrored": false } },
    });
    if let Some(max) = options.max_price.filter(|p| *p > 0.0) {
        let currency = options.currency.as_deref().unwrap_or("exalted");
        if !CURRENCIES.contains(&currency) {
            return Err(crate::Error::Analysis(format!(
                "Unknown currency {currency}"
            )));
        }
        filters["trade_filters"] = serde_json::json!({
            "filters": { "price": { "option": currency, "max": max } }
        });
    }
    let mut query = serde_json::json!({
        "query": {
            "status": { "option": "available" },
            "stats": [{ "type": "weight", "filters": weights }],
            "filters": filters,
        },
        "sort": { "statgroup.0": "desc" },
    });
    if values.same_base_only() && !options.any_base {
        query["query"]["type"] = serde_json::Value::String(values.slot.base.clone());
    }
    Ok(format!(
        "https://www.pathofexile.com/trade2/search/poe2/{}?q={}",
        encode(league),
        encode(&query.to_string())
    ))
}

/// `v` rounded to `digits` significant digits.
fn round_significant(v: f64, digits: i32) -> f64 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let scale = 10f64.powi(digits - 1 - v.abs().log10().floor() as i32);
    (v * scale).round() / scale
}

/// Percent-encodes a query value: everything but unreserved characters and
/// the sub-delimiters that carry no meaning in a query (RFC 3986), so the
/// link stays short. `&`, `=`, `+`, `#` and `%` are always encoded.
fn encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    for b in text.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~:,@!$'()*;/".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn values() -> SlotValues {
        let stat = |id: &str, value: f64, damage: f64, ehp: f64| StatWorth {
            trade_id: id.into(),
            line: String::new(),
            value,
            implicit: false,
            share: Share {
                damage,
                ehp,
                min_hit: 0.0,
            },
        };
        SlotValues {
            slot: GearSlot {
                slot: "Ring 1".into(),
                item: "Skull Loop".into(),
                base: "Unset Ring".into(),
                rarity: "RARE".into(),
                category: "Ring".into(),
                trade_category: "accessory.ring".into(),
            },
            uses_full_dps: false,
            worn_item: Share::default(),
            stats: vec![
                stat("explicit.stat_life", 80.0, 0.0, 4.0),
                stat("explicit.stat_damage", 20.0, 6.0, 0.0),
                stat("explicit.stat_noise", 10.0, 0.01, 0.0),
            ],
            measured: 3,
        }
    }

    fn query(url: &str) -> serde_json::Value {
        let q = url.split_once("?q=").unwrap().1;
        let mut bytes = Vec::new();
        let raw = q.as_bytes();
        let mut i = 0;
        while i < raw.len() {
            if raw[i] == b'%' {
                bytes.push(u8::from_str_radix(&q[i + 1..i + 3], 16).unwrap());
                i += 3;
            } else {
                bytes.push(raw[i]);
                i += 1;
            }
        }
        serde_json::from_slice(&bytes).unwrap()
    }

    #[test]
    fn weights_are_share_per_point_for_the_goal() {
        let options = TradeOptions {
            league: "Rise of the Abyssal".into(),
            goal: Goal::Damage,
            max_price: Some(2.0),
            currency: Some("divine".into()),
            any_base: false,
        };
        let url = trade_url(&values(), &options).unwrap();
        assert!(url.starts_with(
            "https://www.pathofexile.com/trade2/search/poe2/Rise%20of%20the%20Abyssal?q="
        ));
        let q = query(&url);
        let filters = &q["query"]["stats"][0]["filters"];
        assert_eq!(filters.as_array().unwrap().len(), 1, "{filters}");
        assert_eq!(filters[0]["id"], "explicit.stat_damage");
        assert_eq!(filters[0]["value"]["weight"], 0.3);
        assert_eq!(round_significant(0.012_345, 3), 0.0123);
        assert_eq!(round_significant(171.26, 3), 171.0);
        assert_eq!(
            q["query"]["filters"]["trade_filters"]["filters"]["price"]["max"],
            2.0
        );
        assert_eq!(
            q["query"]["filters"]["type_filters"]["filters"]["category"]["option"],
            "accessory.ring"
        );

        let survival = trade_url(
            &values(),
            &TradeOptions {
                goal: Goal::Survival,
                max_price: None,
                ..options
            },
        )
        .unwrap();
        let q = query(&survival);
        assert_eq!(
            q["query"]["stats"][0]["filters"][0]["id"],
            "explicit.stat_life"
        );
        assert!(q["query"]["filters"].get("trade_filters").is_none());
        // Jewellery searches every base.
        assert!(q["query"].get("type").is_none());
    }

    #[test]
    fn armour_and_weapons_keep_to_the_worn_base() {
        let mut v = values();
        v.slot.trade_category = "armour.chest".into();
        v.slot.base = "Slipstrike Vest".into();
        let mut options = TradeOptions {
            league: "Standard".into(),
            goal: Goal::Damage,
            max_price: None,
            currency: None,
            any_base: false,
        };
        assert_eq!(
            query(&trade_url(&v, &options).unwrap())["query"]["type"],
            "Slipstrike Vest"
        );
        options.any_base = true;
        assert!(
            query(&trade_url(&v, &options).unwrap())["query"]
                .get("type")
                .is_none()
        );
    }

    #[test]
    fn refuses_what_it_cannot_search() {
        let mut options = TradeOptions {
            league: " ".into(),
            goal: Goal::Damage,
            max_price: None,
            currency: None,
            any_base: false,
        };
        assert!(trade_url(&values(), &options).is_err());
        options.league = "Standard".into();
        options.max_price = Some(1.0);
        options.currency = Some("mirror".into());
        assert!(trade_url(&values(), &options).is_err());
    }

    #[test]
    fn shares_ignore_immunities() {
        assert_eq!(pct_of(f64::INFINITY, 1.0, 2.0), 0.0);
        assert_eq!(pct_of(200.0, 100.0, 150.0), 25.0);
    }
}
