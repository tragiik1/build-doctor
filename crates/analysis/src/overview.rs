//! A build as PoB sees it.

use pob::{Pool, StatKind, StatSheet};
use serde::Serialize;

use crate::{AssumptionRow, BuildInfo, Result};

/// Sidebar stats worth a glance, grouped: (group, [(PoB stat, label)]).
const GROUPS: &[(&str, &[(&str, &str)])] = &[
    (
        "Offence",
        &[
            ("FullDPS", "Full DPS"),
            ("CombinedDPS", "Combined DPS"),
            ("TotalDPS", "Hit DPS"),
            ("AverageHit", "Average hit"),
            ("Speed", "Uses per second"),
            ("CritChance", "Crit chance"),
            ("HitChance", "Hit chance"),
        ],
    ),
    (
        "Pools",
        &[
            ("Life", "Life"),
            ("EnergyShield", "Energy shield"),
            ("Mana", "Mana"),
            ("Spirit", "Spirit"),
            ("TotalEHP", "Total EHP"),
        ],
    ),
    (
        "Largest hit survived",
        &[
            ("PhysicalMaximumHitTaken", "Physical"),
            ("FireMaximumHitTaken", "Fire"),
            ("ColdMaximumHitTaken", "Cold"),
            ("LightningMaximumHitTaken", "Lightning"),
            ("ChaosMaximumHitTaken", "Chaos"),
        ],
    ),
    (
        "Resistances",
        &[
            ("FireResist", "Fire"),
            ("ColdResist", "Cold"),
            ("LightningResist", "Lightning"),
            ("ChaosResist", "Chaos"),
        ],
    ),
    (
        "Mitigation",
        &[
            ("Armour", "Armour"),
            ("Evasion", "Evasion"),
            ("EffectiveBlockChance", "Block chance"),
            ("EffectiveSpellBlockChance", "Spell block chance"),
        ],
    ),
];

/// Every stat the overview shows: (PoB stat, group, label).
pub fn shown_stats() -> impl Iterator<Item = (&'static str, &'static str, &'static str)> {
    GROUPS
        .iter()
        .flat_map(|(group, stats)| stats.iter().map(move |(key, label)| (*key, *group, *label)))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Overview {
    pub build: BuildInfo,
    pub groups: Vec<StatGroup>,
    pub minion_dps: Option<f64>,
    pub assumptions: Vec<AssumptionRow>,
    pub unsupported: Vec<UnsupportedRow>,
    pub warnings: Vec<String>,
    pub reproduction: Reproduction,
    /// The build as PoB saved it after calculating.
    #[serde(skip)]
    pub xml: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatGroup {
    pub title: &'static str,
    pub rows: Vec<StatRow>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatRow {
    pub stat: &'static str,
    pub label: &'static str,
    pub value: f64,
    /// Exactly as PoB wrote it.
    pub exact: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UnsupportedRow {
    pub slot: String,
    pub item: String,
    pub line: String,
    /// Charm and flask lines: PoB does not model their triggers at all.
    pub usable: bool,
}

/// Whether PoB reproduces the numbers saved in the build.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Reproduction {
    /// Stats the build had saved (0 for builds without saved stats).
    pub saved: usize,
    /// Saved stats PoB now calculates differently, as "stat: saved → now".
    pub differences: Vec<String>,
}

/// Loads `xml` in the pool and reads everything the overview shows.
pub fn overview(pool: &Pool, xml: &str) -> Result<Overview> {
    let saved = StatSheet::from_build_xml(xml)?;
    let mut found = pool.map(xml, None, vec![()], |engine, ()| {
        Ok((
            engine.identity()?,
            engine.assumptions()?,
            engine.unsupported_mods()?,
            engine.warnings()?,
            engine.ensure_original()?,
        ))
    })?;
    let (id, assumptions, unsupported, warnings, saved_now) =
        found.pop().expect("one item in, one out");
    let stats = StatSheet::from_build_xml(&saved_now)?;

    let groups = GROUPS
        .iter()
        .filter_map(|(title, names)| {
            let rows: Vec<StatRow> = names
                .iter()
                .filter_map(|(stat, label)| {
                    let s = stats.get(StatKind::Player, stat)?;
                    let value = s.number()?;
                    // Full DPS is 0 for builds that do not use it.
                    (*stat != "FullDPS" || value != 0.0).then(|| StatRow {
                        stat,
                        label,
                        value,
                        exact: s.value.clone(),
                    })
                })
                .collect();
            (!rows.is_empty()).then_some(StatGroup { title, rows })
        })
        .collect();

    let differences = saved
        .diff(&stats)
        .into_iter()
        .filter(pob::StatDiff::is_mismatch)
        .map(|d| d.to_string())
        .collect();

    Ok(Overview {
        build: BuildInfo::new(&id, &pool_version(pool)?),
        groups,
        minion_dps: stats
            .get(StatKind::Minion, "CombinedDPS")
            .and_then(pob::Stat::number)
            .filter(|v| *v != 0.0),
        assumptions: assumptions.iter().map(AssumptionRow::from).collect(),
        unsupported: unsupported
            .into_iter()
            .map(|m| {
                let usable = m.slot.starts_with("Charm") || m.slot.starts_with("Flask");
                UnsupportedRow {
                    slot: m.slot,
                    item: m.item,
                    line: m.line,
                    usable,
                }
            })
            .collect(),
        warnings,
        reproduction: Reproduction {
            saved: saved.len(),
            differences,
        },
        xml: saved_now,
    })
}

fn pool_version(pool: &Pool) -> Result<String> {
    Ok(pool.install().version().to_string())
}
