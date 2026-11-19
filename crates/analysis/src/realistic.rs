//! What a build's numbers are without the combat conditions it assumes.
//!
//! Nothing here judges which conditions are realistic. PoB recalculates the
//! build with each condition switched off on its own (what that condition is
//! worth), then with all of them off (the floor), then with all but the ones
//! the player keeps. The fight simulated (enemy type, resistance penalty,
//! calculation modes) and campaign rewards stay as configured.

use pob::{Pool, StatKind, StatSheet};
use serde::Serialize;

use crate::{AssumptionRow, BuildInfo, Result, check_kept, conditions, inspect, pct};

/// A number the analysis tracks.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Metric {
    pub key: &'static str,
    pub label: &'static str,
    /// Damage (true) or survival (false).
    pub damage: bool,
}

const METRICS: &[Metric] = &[
    Metric {
        key: "fullDps",
        label: "Full DPS",
        damage: true,
    },
    Metric {
        key: "combinedDps",
        label: "Combined DPS",
        damage: true,
    },
    Metric {
        key: "minionDps",
        label: "Minion DPS",
        damage: true,
    },
    Metric {
        key: "ehp",
        label: "Total EHP",
        damage: false,
    },
    Metric {
        key: "minHit",
        label: "Smallest max hit",
        damage: false,
    },
];
const DAMAGE_TYPES: &[&str] = &["Physical", "Fire", "Cold", "Lightning", "Chaos"];

impl Metric {
    fn value(self, stats: &StatSheet) -> Option<f64> {
        match self.key {
            "fullDps" => stats.player("FullDPS"),
            "combinedDps" => stats.player("CombinedDPS"),
            "minionDps" => stats
                .get(StatKind::Minion, "CombinedDPS")
                .and_then(pob::Stat::number),
            "ehp" => stats.player("TotalEHP"),
            _ => smallest_max_hit(stats).map(|(v, _)| v),
        }
    }
}

/// The smallest of the five "maximum hit taken" values, with its type.
pub fn smallest_max_hit(stats: &StatSheet) -> Option<(f64, &'static str)> {
    DAMAGE_TYPES
        .iter()
        .filter_map(|t| {
            stats
                .player(&format!("{t}MaximumHitTaken"))
                .map(|v| (v, *t))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Realistic {
    pub build: BuildInfo,
    /// The metrics this build produces; every value list below follows it.
    pub metrics: Vec<Metric>,
    pub configured: Vec<f64>,
    /// All conditions off.
    pub floor: Vec<f64>,
    /// All conditions off except the kept ones (when any are kept).
    pub kept: Option<Vec<f64>>,
    /// Damage type of the smallest max hit: configured, floor, kept.
    pub min_hit_type: [Option<&'static str>; 3],
    pub conditions: Vec<ConditionImpact>,
    pub scenario: Vec<AssumptionRow>,
}

/// What switching one condition off does, in percent per metric.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConditionImpact {
    pub condition: AssumptionRow,
    pub changes: Vec<f64>,
    pub kept: bool,
}

/// `kept`: PoB config variables of conditions the player keeps up.
pub fn realistic(pool: &Pool, xml: &str, kept: &[String]) -> Result<Realistic> {
    let found = inspect(pool, xml)?;
    let conditions = conditions(&found.assumptions);
    check_kept(&conditions, kept)?;
    let base = &found.stats;

    let metrics: Vec<Metric> = METRICS
        .iter()
        .copied()
        .filter(|m| m.value(base).is_some_and(|v| v != 0.0))
        .collect();
    if metrics.is_empty() {
        return Err(crate::Error::Analysis(
            "PoB produced no damage or survival numbers for this build".into(),
        ));
    }

    let mut runs: Vec<Vec<String>> = conditions.iter().map(|c| vec![c.var.clone()]).collect();
    runs.push(conditions.iter().map(|c| c.var.clone()).collect());
    if !kept.is_empty() {
        runs.push(
            conditions
                .iter()
                .filter(|c| !kept.contains(&c.var))
                .map(|c| c.var.clone())
                .collect(),
        );
    }
    let mut results = pool.map(xml, Some(base), runs, |engine, vars| {
        let vars: Vec<&str> = vars.iter().map(String::as_str).collect();
        engine.without_config(&vars).map(|calc| calc.stats)
    })?;
    let kept_stats = if kept.is_empty() { None } else { results.pop() };
    let floor = results.pop().expect("floor run is always queued");

    let values = |stats: &StatSheet| {
        metrics
            .iter()
            .map(|m| m.value(stats).unwrap_or(0.0))
            .collect::<Vec<_>>()
    };
    let configured = values(base);
    let impacts = conditions
        .iter()
        .zip(&results)
        .map(|(c, stats)| ConditionImpact {
            condition: AssumptionRow::from(*c),
            changes: values(stats)
                .iter()
                .zip(&configured)
                .map(|(after, before)| pct(*before, *after))
                .collect(),
            kept: kept.contains(&c.var),
        })
        .collect();

    Ok(Realistic {
        build: BuildInfo::new(&found.id, &pool.install().version().to_string()),
        min_hit_type: [
            smallest_max_hit(base).map(|(_, t)| t),
            smallest_max_hit(&floor).map(|(_, t)| t),
            kept_stats
                .as_ref()
                .and_then(smallest_max_hit)
                .map(|(_, t)| t),
        ],
        floor: values(&floor),
        kept: kept_stats.as_ref().map(values),
        configured,
        metrics,
        conditions: impacts,
        scenario: found
            .assumptions
            .iter()
            .filter(|a| a.kind == pob::AssumptionKind::Scenario && a.section != "Quest Rewards")
            .map(AssumptionRow::from)
            .collect(),
    })
}
