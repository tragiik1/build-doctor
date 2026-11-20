//! "Is this item an upgrade?": an item's text, equipped in each slot it fits
//! instead of what the build wears there, with PoB recalculating fully.
//!
//! The item is used exactly as pasted (quality and rolls unchanged), and the
//! build is restored and checked byte-identical after every trial.

use pob::{Pool, StatSheet};
use serde::Serialize;

use crate::diagnose::{Effect, damage_effect, measured, new_warnings};
use crate::{BuildInfo, Result};

/// Stats shown when they change, after damage, Total EHP and the smallest
/// max hit: (label, PoB stat).
const SHOWN: &[(&str, &str)] = &[
    ("Life", "Life"),
    ("Energy shield", "EnergyShield"),
    ("Mana", "Mana"),
    ("Spirit", "Spirit"),
    ("Fire resistance", "FireResist"),
    ("Cold resistance", "ColdResist"),
    ("Lightning resistance", "LightningResist"),
    ("Chaos resistance", "ChaosResist"),
    ("Physical max hit", "PhysicalMaximumHitTaken"),
    ("Fire max hit", "FireMaximumHitTaken"),
    ("Cold max hit", "ColdMaximumHitTaken"),
    ("Lightning max hit", "LightningMaximumHitTaken"),
    ("Chaos max hit", "ChaosMaximumHitTaken"),
    ("Armour", "Armour"),
    ("Evasion", "Evasion"),
    ("Block chance", "EffectiveBlockChance"),
    ("Strength", "Str"),
    ("Dexterity", "Dex"),
    ("Intelligence", "Int"),
];

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemCheck {
    pub build: BuildInfo,
    pub item: String,
    pub rarity: String,
    /// Lines of the item PoB cannot read; no number includes them.
    pub unread: Vec<String>,
    /// One per slot the item fits, best damage change first.
    pub slots: Vec<SlotResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotResult {
    pub slot: String,
    /// What to call the slot ("Ring 1", "Jewel socket 2").
    pub label: String,
    /// What the build wears there now.
    pub replaces: Option<String>,
    /// "better", "worse" or "mixed", from damage and Total EHP.
    pub verdict: &'static str,
    /// Damage, Total EHP and smallest max hit, always shown.
    pub headline: Vec<Effect>,
    /// Every other shown stat that changes.
    pub changes: Vec<Effect>,
    /// PoB warnings the item adds (an unmet requirement, for example).
    pub new_warnings: Vec<String>,
}

pub fn item_check(pool: &Pool, xml: &str, item: &str) -> Result<ItemCheck> {
    // The Windows clipboard ends lines with CRLF; PoB reads LF.
    let item = item
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .trim()
        .to_owned();
    let raw = item.clone();
    let mut found = pool.map(xml, None, vec![()], move |engine, ()| {
        Ok((
            engine.identity()?,
            engine.item_info(&raw)?,
            engine.warnings()?,
            engine.skill_usage()?,
            engine.ensure_original()?,
        ))
    })?;
    let (id, info, warnings, skills, saved) = found.pop().expect("one item in, one out");
    let base = StatSheet::from_build_xml(&saved)?;

    let raw = item.clone();
    let tried = pool.map(
        xml,
        Some(&base),
        info.slots.iter().map(|s| s.slot.clone()).collect(),
        move |engine, slot| engine.with_item(&raw, &slot),
    )?;

    let mut slots: Vec<SlotResult> = info
        .slots
        .iter()
        .zip(tried)
        .map(|(slot, (calc, after_warnings))| {
            let (label, before, after) = damage_effect(&base, &calc.stats);
            let mut headline = vec![Effect::new(label, before, after)];
            headline.extend(measured(
                &[("Total EHP".into(), "TotalEHP".into())],
                &base,
                &calc.stats,
            ));
            if let (Some(b), Some(a)) = (min_hit(&base), min_hit(&calc.stats)) {
                headline.push(Effect::new("Smallest max hit", b, a));
            }
            let shown: Vec<(String, String)> = SHOWN
                .iter()
                .map(|(l, k)| ((*l).to_owned(), (*k).to_owned()))
                .collect();
            SlotResult {
                slot: slot.slot.clone(),
                label: slot.label.clone(),
                replaces: slot.current.clone(),
                verdict: verdict(&headline),
                changes: measured(&shown, &base, &calc.stats),
                new_warnings: new_warnings(&warnings, &after_warnings, &skills),
                headline,
            }
        })
        .collect();
    // Best damage change first, then best Total EHP change.
    let ehp = |s: &SlotResult| {
        s.headline
            .iter()
            .find(|e| e.label == "Total EHP")
            .map_or(0.0, |e| e.change)
    };
    slots.sort_by(|a, b| {
        b.headline[0]
            .change
            .total_cmp(&a.headline[0].change)
            .then_with(|| ehp(b).total_cmp(&ehp(a)))
    });

    Ok(ItemCheck {
        build: BuildInfo::new(&id, &pool.install().version().to_string()),
        item: info.name,
        rarity: info.rarity,
        unread: info.unread,
        slots,
    })
}

fn min_hit(stats: &StatSheet) -> Option<f64> {
    ["Physical", "Fire", "Cold", "Lightning", "Chaos"]
        .iter()
        .filter_map(|t| stats.player(&format!("{t}MaximumHitTaken")))
        .filter(|v| v.is_finite())
        .reduce(f64::min)
}

/// From the damage and Total EHP changes (the first two headline effects).
fn verdict(headline: &[Effect]) -> &'static str {
    let changes: Vec<f64> = headline
        .iter()
        .take(2)
        .map(|e| e.change)
        .filter(|c| c.abs() >= 0.05)
        .collect();
    if changes.is_empty() {
        "same"
    } else if changes.iter().all(|c| *c > 0.0) {
        "better"
    } else if changes.iter().all(|c| *c < 0.0) {
        "worse"
    } else {
        "mixed"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn effects(changes: &[f64]) -> Vec<Effect> {
        changes
            .iter()
            .map(|c| Effect::new("x", 100.0, 100.0 + c))
            .collect()
    }

    #[test]
    fn verdicts_from_damage_and_ehp() {
        assert_eq!(verdict(&effects(&[5.0, 1.0])), "better");
        assert_eq!(verdict(&effects(&[5.0, 0.0])), "better");
        assert_eq!(verdict(&effects(&[-5.0, -1.0])), "worse");
        assert_eq!(verdict(&effects(&[5.0, -1.0])), "mixed");
        assert_eq!(verdict(&effects(&[0.0, 0.01])), "same");
    }
}
