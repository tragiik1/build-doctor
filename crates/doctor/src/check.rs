//! `doctor check`: calculate one build and compare with its saved numbers.

use std::process::ExitCode;

use pob::{Pool, StatSheet};

use crate::AnyResult;

pub fn run(pool: &Pool, xml: &str, all: bool) -> AnyResult<ExitCode> {
    let o = analysis::overview(pool, xml)?;
    println!("{}", o.build.describe());
    println!(
        "PoB2 {} (tree {})",
        o.build.pob_version,
        o.build.tree_version.as_deref().unwrap_or("-")
    );
    if o.build.tree_outdated {
        println!(
            "Warning: the build uses passive tree {}, but this PoB's newest tree is {}. \
             PoB calculates it on the old tree, so these numbers describe an older game version.",
            o.build.tree_version.as_deref().unwrap_or("-"),
            o.build.latest_tree_version.as_deref().unwrap_or("-"),
        );
    }
    println!();

    if all {
        for stat in StatSheet::from_build_xml(&o.xml)?.iter() {
            println!("  {:<8} {:<40} {}", stat.kind, stat.name, stat.value);
        }
    } else {
        for group in &o.groups {
            println!("{}", group.title);
            for row in &group.rows {
                println!("  {:<28} {:>14}", row.label, format_number(row.value));
            }
        }
        if let Some(minion) = o.minion_dps {
            println!(
                "Minions\n  {:<28} {:>14}",
                "Combined DPS",
                format_number(minion)
            );
        }
    }
    println!();

    if !o.unsupported.is_empty() {
        println!("Not counted (PoB cannot parse these mods, so no number above includes them)");
        let usables = o.unsupported.iter().filter(|m| m.usable).count();
        for m in o.unsupported.iter().filter(|m| all || !m.usable) {
            println!("  {:<18} {}  ({})", m.slot, m.line, m.item);
        }
        if !all && usables > 0 {
            println!("  Charms and flasks: {usables} more lines (--all lists them)");
        }
        println!();
    }

    if !o.assumptions.is_empty() {
        println!("Assumes (PoB configuration these numbers depend on)");
        // Campaign rewards are facts about the character, not combat
        // assumptions; list them only on request.
        let facts = o
            .assumptions
            .iter()
            .filter(|a| a.section == "Quest Rewards")
            .count();
        let mut section = "";
        for a in o
            .assumptions
            .iter()
            .filter(|a| all || a.section != "Quest Rewards")
        {
            if a.section != section {
                section = &a.section;
                println!("  {section}");
            }
            println!("    {:<56} {}", a.label, one_line(&a.value));
        }
        if !all && facts > 0 {
            println!("  Quest Rewards: {facts} set (--all lists them)");
        }
        println!();
    }

    let r = &o.reproduction;
    if r.saved == 0 {
        println!("The build has no saved stats to compare against.");
        return Ok(ExitCode::SUCCESS);
    }
    if r.differences.is_empty() {
        println!("All {} saved stats reproduced exactly.", r.saved);
        return Ok(ExitCode::SUCCESS);
    }
    println!(
        "{} of {} saved stats differ. The build was probably saved by a different PoB version; \
         `doctor verify --sweep` finds which one.",
        r.differences.len(),
        r.saved
    );
    for diff in &r.differences {
        println!("  {diff}");
    }
    Ok(ExitCode::from(2))
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

/// Signed percent with one decimal; "0%" below 0.05.
pub fn signed(value: f64) -> String {
    if value.abs() < 0.05 {
        "0%".into()
    } else {
        format!("{value:+.1}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers() {
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(1234567.891), "1,234,568");
        assert_eq!(format_number(12.345), "12.35");
        assert_eq!(format_number(999.5), "999.5");
        assert_eq!(format_number(-42.0), "-42");
        assert_eq!(format_number(-0.001), "0");
        assert_eq!(format_number(100.0), "100");
        assert_eq!(signed(-61.84), "-61.8%");
        assert_eq!(signed(0.01), "0%");
    }
}
