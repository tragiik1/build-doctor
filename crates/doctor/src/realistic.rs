//! `doctor realistic`: what a build's numbers are without the combat
//! conditions it assumes.
//!
//! Nothing here judges which conditions are realistic. PoB recalculates the
//! build with each condition switched off on its own (what that condition is
//! worth), then with all of them off (the floor), then with only the ones the
//! player says they really keep up (`--keep`). The fight being simulated
//! (enemy type, resistance penalty, calculation modes) and campaign rewards
//! stay as configured.

use std::process::ExitCode;
use std::time::Instant;

use pob::{AssumptionKind, Install, Pool, StatKind, StatSheet};

use crate::AnyResult;
use crate::check::{describe_build, format_number};
use crate::conditions::{self, Inspected, Keep};

/// A number the report tracks. Damage metrics first, then survival.
#[derive(Debug, Clone, Copy)]
enum Metric {
    Stat {
        label: &'static str,
        kind: StatKind,
        name: &'static str,
    },
    SmallestMaxHit,
}

const DAMAGE: &[Metric] = &[
    Metric::Stat {
        label: "Full DPS",
        kind: StatKind::Player,
        name: "FullDPS",
    },
    Metric::Stat {
        label: "Combined DPS",
        kind: StatKind::Player,
        name: "CombinedDPS",
    },
    Metric::Stat {
        label: "Minion DPS",
        kind: StatKind::Minion,
        name: "CombinedDPS",
    },
];
const SURVIVAL: &[Metric] = &[
    Metric::Stat {
        label: "Total EHP",
        kind: StatKind::Player,
        name: "TotalEHP",
    },
    Metric::SmallestMaxHit,
];
const DAMAGE_TYPES: &[&str] = &["Physical", "Fire", "Cold", "Lightning", "Chaos"];

impl Metric {
    fn label(self) -> &'static str {
        match self {
            Self::Stat { label, .. } => label,
            Self::SmallestMaxHit => "Smallest max hit",
        }
    }

    fn value(self, stats: &StatSheet) -> Option<f64> {
        match self {
            Self::Stat { kind, name, .. } => stats.get(kind, name).and_then(pob::Stat::number),
            Self::SmallestMaxHit => smallest_max_hit(stats).map(|(v, _)| v),
        }
    }

    fn shown(self, stats: &StatSheet) -> String {
        match self {
            Self::SmallestMaxHit => smallest_max_hit(stats)
                .map_or_else(|| "-".into(), |(v, t)| format!("{} {t}", format_number(v))),
            _ => self.value(stats).map_or_else(|| "-".into(), format_number),
        }
    }
}

fn smallest_max_hit(stats: &StatSheet) -> Option<(f64, &'static str)> {
    DAMAGE_TYPES
        .iter()
        .filter_map(|t| {
            stats
                .player(&format!("{t}MaximumHitTaken"))
                .map(|v| (v, *t))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
}

struct Row {
    label: String,
    value: String,
    changes: Vec<Option<f64>>,
}

pub fn run(install: &Install, xml: &str, keep: &[String], jobs: usize) -> AnyResult<ExitCode> {
    let started = Instant::now();
    let pool = Pool::new(install, jobs)?;
    let Inspected {
        id,
        assumptions,
        calc: base,
    } = conditions::inspect(&pool, xml)?;

    // Only metrics this build actually produces.
    let metrics: Vec<Metric> = DAMAGE
        .iter()
        .chain(SURVIVAL)
        .copied()
        .filter(|m| m.value(&base.stats).is_some_and(|v| v != 0.0))
        .collect();
    if metrics.is_empty() {
        return Err("PoB produced no damage or survival numbers for this build".into());
    }

    let conditions = conditions::conditions(&assumptions);
    let scenario: Vec<_> = assumptions
        .iter()
        .filter(|a| a.kind == AssumptionKind::Scenario)
        .collect();
    let keep = Keep::new(keep, &conditions)?;

    let change = |metric: Metric, stats: &StatSheet| -> Option<f64> {
        let before = metric.value(&base.stats)?;
        let after = metric.value(stats)?;
        Some(if before == 0.0 {
            0.0
        } else {
            (after - before) / before * 100.0
        })
    };

    // Each condition switched off on its own, then all of them, then all but
    // the kept ones.
    let mut runs: Vec<Vec<String>> = conditions.iter().map(|c| vec![c.var.clone()]).collect();
    runs.push(conditions.iter().map(|c| c.var.clone()).collect());
    if !keep.is_empty() {
        runs.push(
            conditions
                .iter()
                .filter(|c| !keep.keeps(&c.label))
                .map(|c| c.var.clone())
                .collect(),
        );
    }
    let mut results = pool.map(xml, Some(&base.stats), runs, |engine, vars| {
        let vars: Vec<&str> = vars.iter().map(String::as_str).collect();
        engine.without_config(&vars).map(|calc| calc.stats)
    })?;
    let realistic = if keep.is_empty() { None } else { results.pop() };
    let floor = results.pop().expect("floor run is always queued");
    let rows: Vec<Row> = conditions
        .iter()
        .zip(&results)
        .map(|(condition, stats)| Row {
            label: condition.label.clone(),
            value: crate::check::one_line(&condition.value),
            changes: metrics.iter().map(|m| change(*m, stats)).collect(),
        })
        .collect();

    // Report.
    println!("{}", describe_build(&id));
    println!(
        "PoB2 {}, {} conditions tested on {} engines in {:.1}s",
        install.version(),
        conditions.len(),
        pool.size(),
        started.elapsed().as_secs_f64()
    );
    println!();

    let last_header = if realistic.is_some() {
        "Keeping yours"
    } else {
        ""
    };
    println!(
        "{:<18} {:>20} {:>24} {:>20}",
        "", "As configured", "No conditions", last_header
    );
    for metric in &metrics {
        let floor_text = with_change(metric.shown(&floor), change(*metric, &floor));
        let real_text = realistic
            .as_ref()
            .map(|r| with_change(metric.shown(r), change(*metric, r)))
            .unwrap_or_default();
        println!(
            "{:<18} {:>20} {:>24} {:>20}",
            metric.label(),
            metric.shown(&base.stats),
            floor_text,
            real_text
        );
    }
    println!();

    if conditions.is_empty() {
        println!("The build assumes no combat conditions; these are its numbers as they stand.");
        return Ok(ExitCode::SUCCESS);
    }

    // Biggest effect first; conditions that change nothing collapse.
    let mut rows = rows;
    rows.sort_by(|a, b| {
        let key = |r: &Row| {
            r.changes
                .iter()
                .flatten()
                .map(|c| c.abs())
                .fold(0.0, f64::max)
        };
        key(b).total_cmp(&key(a))
    });
    let (moving, inert): (Vec<_>, Vec<_>) = rows
        .iter()
        .partition(|r| r.changes.iter().flatten().any(|c| *c != 0.0));

    println!("What each condition is worth (PoB with only that one switched off)");
    print!("  {:<52}", "");
    for metric in &metrics {
        print!(" {:>13}", short_label(*metric));
    }
    println!();
    for row in &moving {
        let mark = if keep.keeps(&row.label) {
            "kept "
        } else {
            "     "
        };
        let label = if row.value == "yes" {
            row.label.clone()
        } else {
            format!("{} = {}", row.label, row.value)
        };
        print!("{mark}{:<49}", truncate(&label, 49));
        for c in &row.changes {
            print!(" {:>13}", c.map_or_else(|| "-".into(), percent));
        }
        println!();
    }
    if !inert.is_empty() {
        println!(
            "  No effect on these numbers: {}",
            inert
                .iter()
                .map(|r| r.label.as_str())
                .collect::<Vec<_>>()
                .join("; ")
        );
    }
    println!("  Conditions overlap, so single rows do not add up to the total.");
    println!();

    let fixed: Vec<String> = scenario
        .iter()
        .filter(|a| a.section != "Quest Rewards")
        .map(|a| format!("{} = {}", a.label, a.value))
        .collect();
    if !fixed.is_empty() {
        println!("Fight simulated (kept as configured): {}", fixed.join("; "));
    }
    if realistic.is_none() {
        println!(
            "Keep the conditions you really sustain, for example: doctor realistic <build> --keep shocked --keep cursed"
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn short_label(metric: Metric) -> &'static str {
    match metric.label() {
        "Smallest max hit" => "Min max hit",
        other => other,
    }
}

fn with_change(value: String, change: Option<f64>) -> String {
    match change {
        Some(c) => format!("{value} ({})", percent(c)),
        None => value,
    }
}

fn percent(change: f64) -> String {
    if change == 0.0 {
        "0%".into()
    } else if change.abs() < 0.05 {
        if change < 0.0 {
            "-<0.1%".into()
        } else {
            "+<0.1%".into()
        }
    } else {
        format!("{change:+.1}%")
    }
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        let cut: String = text.chars().take(max - 1).collect();
        format!("{cut}…")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_formatting() {
        assert_eq!(percent(0.0), "0%");
        assert_eq!(percent(-61.84), "-61.8%");
        assert_eq!(percent(12.0), "+12.0%");
        assert_eq!(percent(-0.01), "-<0.1%");
    }

    #[test]
    fn smallest_max_hit_names_the_type() {
        let stats = StatSheet::from_build_xml(
            r#"<P><Build>
                <PlayerStat stat="FireMaximumHitTaken" value="9000"/>
                <PlayerStat stat="ChaosMaximumHitTaken" value="4000"/>
                <PlayerStat stat="PhysicalMaximumHitTaken" value="5000"/>
            </Build></P>"#,
        )
        .unwrap();
        assert_eq!(smallest_max_hit(&stats), Some((4000.0, "Chaos")));
    }
}
