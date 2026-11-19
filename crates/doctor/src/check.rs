//! `doctor check`: calculate one build and compare with its saved numbers.

use std::process::ExitCode;
use std::time::Instant;

use pob::{Engine, Install, StatKind, StatSheet};

use crate::AnyResult;

/// Sidebar stats worth a glance, grouped. Only those PoB produced are shown.
const SUMMARY: &[(&str, &[&str])] = &[
    (
        "Offence",
        &[
            "CombinedDPS",
            "TotalDPS",
            "AverageHit",
            "Speed",
            "CritChance",
            "HitChance",
        ],
    ),
    (
        "Pools",
        &["Life", "EnergyShield", "Mana", "Spirit", "TotalEHP"],
    ),
    (
        "Largest hit survived",
        &[
            "PhysicalMaximumHitTaken",
            "FireMaximumHitTaken",
            "ColdMaximumHitTaken",
            "LightningMaximumHitTaken",
            "ChaosMaximumHitTaken",
        ],
    ),
    (
        "Resistances",
        &["FireResist", "ColdResist", "LightningResist", "ChaosResist"],
    ),
    (
        "Mitigation",
        &[
            "Armour",
            "Evasion",
            "EffectiveBlockChance",
            "EffectiveSpellBlockChance",
        ],
    ),
];

pub fn run(install: &Install, xml: &str, all: bool) -> AnyResult<ExitCode> {
    let saved = StatSheet::from_build_xml(xml)?;

    let started = Instant::now();
    let engine = Engine::start(install)?;
    let startup = started.elapsed();
    let started = Instant::now();
    let calc = engine.load(xml)?;
    let load = started.elapsed();
    let id = engine.identity()?;

    let or_dash = |v: &Option<String>| v.clone().unwrap_or_else(|| "-".into());
    let who = match (&id.ascendancy, &id.class) {
        (Some(asc), Some(class)) if asc != "None" => format!("{asc} ({class})"),
        (_, Some(class)) => class.clone(),
        _ => "Unknown class".into(),
    };
    println!(
        "{who}, level {}, main skill {}",
        id.level.map_or("-".into(), |l| l.to_string()),
        or_dash(&id.main_skill),
    );
    println!(
        "PoB2 {} (tree {}), started in {:.1}s, calculated in {:.2}s over {} pass{}",
        install.version(),
        or_dash(&id.tree_version),
        startup.as_secs_f64(),
        load.as_secs_f64(),
        calc.passes,
        if calc.passes == 1 { "" } else { "es" },
    );
    println!();

    if all {
        for stat in calc.stats.iter() {
            println!("  {:<8} {:<40} {}", stat.kind, stat.name, stat.value);
        }
    } else {
        for (group, names) in SUMMARY {
            let rows: Vec<_> = names
                .iter()
                .filter_map(|n| calc.stats.get(StatKind::Player, n).map(|s| (n, s)))
                .collect();
            if rows.is_empty() {
                continue;
            }
            println!("{group}");
            for (name, stat) in rows {
                let shown = stat
                    .number()
                    .map_or_else(|| stat.value.clone(), format_number);
                println!("  {name:<28} {shown:>14}");
            }
        }
    }
    println!();

    if saved.is_empty() {
        println!("The build has no saved stats to compare against.");
        return Ok(ExitCode::SUCCESS);
    }
    let diffs = saved.diff(&calc.stats);
    let mismatches: Vec<_> = diffs.iter().filter(|d| d.is_mismatch()).collect();
    if mismatches.is_empty() {
        println!("All {} saved stats reproduced exactly.", saved.len());
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{} of {} saved stats differ. The build was probably saved by a different PoB version; \
         `doctor verify --sweep` finds which one.",
        mismatches.len(),
        saved.len()
    );
    for diff in mismatches {
        println!("  {diff}");
    }
    Ok(ExitCode::from(2))
}

/// Thousands separators, at most two decimals, trailing zeros dropped.
pub fn format_number(value: f64) -> String {
    let rounded = format!("{:.2}", value.abs());
    let (int, frac) = rounded.split_once('.').unwrap_or((&rounded, ""));
    let mut grouped = String::new();
    for (i, digit) in int.chars().enumerate() {
        if i > 0 && (int.len() - i) % 3 == 0 {
            grouped.push(',');
        }
        grouped.push(digit);
    }
    let frac = frac.trim_end_matches('0');
    let sign = if value < 0.0 && rounded.chars().any(|c| c.is_ascii_digit() && c != '0') {
        "-"
    } else {
        ""
    };
    if frac.is_empty() {
        format!("{sign}{grouped}")
    } else {
        format!("{sign}{grouped}.{frac}")
    }
}

#[cfg(test)]
mod tests {
    use super::format_number;

    #[test]
    fn formats_numbers() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(1234567.891), "1,234,567.89");
        assert_eq!(format_number(999.5), "999.5");
        assert_eq!(format_number(-42.0), "-42");
        assert_eq!(format_number(-0.001), "0");
        assert_eq!(format_number(100.0), "100");
    }
}
