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
    println!("{}", describe_build(&id));
    println!(
        "PoB2 {} (tree {}), started in {:.1}s, calculated in {:.2}s over {} pass{}",
        install.version(),
        or_dash(&id.tree_version),
        startup.as_secs_f64(),
        load.as_secs_f64(),
        calc.passes,
        if calc.passes == 1 { "" } else { "es" },
    );
    if id.tree_is_outdated() {
        println!(
            "Warning: the build uses passive tree {}, but this PoB's newest tree is {}. \
             PoB calculates it on the old tree, so these numbers describe an older game version.",
            or_dash(&id.tree_version),
            or_dash(&id.latest_tree_version),
        );
    }
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

    let unsupported = engine.unsupported_mods()?;
    if !unsupported.is_empty() {
        // PoB does not model charm and flask trigger lines at all; they are
        // listed on request so the gear lines that matter stay visible.
        let (usables, gear): (Vec<_>, Vec<_>) = unsupported
            .iter()
            .partition(|m| m.slot.starts_with("Charm") || m.slot.starts_with("Flask"));
        println!("Not counted (PoB cannot parse these mods, so no number above includes them)");
        for m in gear
            .iter()
            .chain(if all { usables.iter() } else { [].iter() })
        {
            println!("  {:<18} {}  ({})", m.slot, m.line, m.item);
        }
        if !all && !usables.is_empty() {
            println!(
                "  Charms and flasks: {} more lines (--all lists them)",
                usables.len()
            );
        }
        println!();
    }

    let assumptions = engine.assumptions()?;
    if !assumptions.is_empty() {
        println!("Assumes (PoB configuration these numbers depend on)");
        // Campaign rewards are facts about the character, not combat
        // assumptions; list them only on request.
        let (facts, conditions): (Vec<_>, Vec<_>) = assumptions
            .iter()
            .partition(|a| a.section == "Quest Rewards");
        let mut section = "";
        for a in conditions {
            if a.section != section {
                section = &a.section;
                println!("  {section}");
            }
            let label = if a.label.is_empty() {
                "(text)"
            } else {
                &a.label
            };
            println!("    {label:<56} {}", one_line(&a.value));
        }
        if !facts.is_empty() && !all {
            println!("  Quest Rewards: {} set (--all lists them)", facts.len());
        } else if !facts.is_empty() {
            println!("  Quest Rewards");
            for a in facts {
                println!("    {:<56} {}", a.label, one_line(&a.value));
            }
        }
        println!();
    }

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

/// "Shaman (Druid), level 94, main skill Snap".
pub fn describe_build(id: &pob::Identity) -> String {
    let who = match (&id.ascendancy, &id.class) {
        (Some(asc), Some(class)) if asc != "None" => format!("{asc} ({class})"),
        (_, Some(class)) => class.clone(),
        _ => "Unknown class".into(),
    };
    format!(
        "{who}, level {}, main skill {}",
        id.level.map_or("-".into(), |l| l.to_string()),
        id.main_skill.as_deref().unwrap_or("-"),
    )
}

/// Multi-line config values (custom modifiers, quest rewards) on one line.
pub fn one_line(value: &str) -> String {
    value
        .split(['\n', '\t'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("; ")
}

/// Thousands separators; whole numbers from 1,000 up, otherwise at most two
/// decimals, trailing zeros dropped. Display only: PoB's exact values are in
/// `check --all`.
pub fn format_number(value: f64) -> String {
    let decimals = if value.abs() >= 1000.0 { 0 } else { 2 };
    let rounded = format!("{:.*}", decimals, value.abs());
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
        assert_eq!(format_number(1234567.891), "1,234,568");
        assert_eq!(format_number(12.345), "12.35");
        assert_eq!(format_number(999.5), "999.5");
        assert_eq!(format_number(-42.0), "-42");
        assert_eq!(format_number(-0.001), "0");
        assert_eq!(format_number(100.0), "100");
    }
}
