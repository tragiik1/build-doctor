//! `doctor realistic`: prints [`analysis::realistic`].

use std::process::ExitCode;

use pob::Pool;

use crate::AnyResult;
use crate::check::{format_number, one_line, signed};

pub fn run(pool: &Pool, xml: &str, kept: &[String]) -> AnyResult<ExitCode> {
    let r = analysis::realistic(pool, xml, kept)?;
    println!("{}", r.build.describe());
    println!(
        "PoB2 {}, {} conditions tested",
        r.build.pob_version,
        r.conditions.len()
    );
    println!();

    let shown = |i: usize, values: &[f64], column: usize| {
        let mut text = format_number(values[i]);
        if r.metrics[i].key == "minHit"
            && let Some(t) = r.min_hit_type[column]
        {
            text = format!("{text} {t}");
        }
        text
    };
    let with_change = |i: usize, values: &[f64], column: usize| {
        format!(
            "{} ({})",
            shown(i, values, column),
            signed(analysis::pct(r.configured[i], values[i]))
        )
    };
    let last = if r.kept.is_some() {
        "Keeping yours"
    } else {
        ""
    };
    println!(
        "{:<18} {:>20} {:>24} {:>24}",
        "", "As configured", "No conditions", last
    );
    for (i, metric) in r.metrics.iter().enumerate() {
        println!(
            "{:<18} {:>20} {:>24} {:>24}",
            metric.label,
            shown(i, &r.configured, 0),
            with_change(i, &r.floor, 1),
            r.kept
                .as_ref()
                .map(|k| with_change(i, k, 2))
                .unwrap_or_default(),
        );
    }
    println!();

    if r.conditions.is_empty() {
        println!("The build assumes no combat conditions; these are its numbers as they stand.");
        return Ok(ExitCode::SUCCESS);
    }
    let mut rows: Vec<_> = r.conditions.iter().collect();
    let size = |c: &analysis::realistic::ConditionImpact| {
        c.changes.iter().map(|v| v.abs()).fold(0.0, f64::max)
    };
    rows.sort_by(|a, b| size(b).total_cmp(&size(a)));
    let (moving, inert): (Vec<_>, Vec<_>) = rows.into_iter().partition(|c| size(c) > 0.0);

    println!("What each condition is worth (PoB with only that one switched off)");
    print!("  {:<52}", "");
    for metric in &r.metrics {
        print!(
            " {:>13}",
            metric.label.replace("Smallest max hit", "Min max hit")
        );
    }
    println!();
    for c in &moving {
        let a = &c.condition;
        let label = if a.value == "yes" {
            a.label.clone()
        } else {
            format!("{} = {}", a.label, one_line(&a.value))
        };
        print!(
            "{}{:<49}",
            if c.kept { "kept " } else { "     " },
            truncate(&label, 49)
        );
        for change in &c.changes {
            print!(" {:>13}", signed(*change));
        }
        println!();
    }
    if !inert.is_empty() {
        let labels: Vec<&str> = inert.iter().map(|c| c.condition.label.as_str()).collect();
        println!("  No effect on these numbers: {}", labels.join("; "));
    }
    println!("  Conditions overlap, so single rows do not add up to the total.");
    println!();
    if !r.scenario.is_empty() {
        let fixed: Vec<String> = r
            .scenario
            .iter()
            .map(|a| format!("{} = {}", a.label, a.value))
            .collect();
        println!("Fight simulated (kept as configured): {}", fixed.join("; "));
    }
    if r.kept.is_none() {
        println!(
            "Keep the conditions you really sustain, for example: doctor realistic <build> --keep shocked --keep cursed"
        );
    }
    Ok(ExitCode::SUCCESS)
}

fn truncate(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_owned()
    } else {
        format!("{}…", text.chars().take(max - 1).collect::<String>())
    }
}
