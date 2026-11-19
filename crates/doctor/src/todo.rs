//! `doctor todo`: prints [`analysis::plan`].

use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use analysis::{Goal, PlanOptions, damage, pct};
use pob::{NodeRef, Pool};

use crate::AnyResult;
use crate::check::{format_number, signed};

pub fn run(
    pool: &Pool,
    xml: &str,
    options: &PlanOptions,
    export: Option<PathBuf>,
) -> AnyResult<ExitCode> {
    let plan = analysis::plan(pool, xml, options, &|_| {}, &|| false)?;
    let full = plan.uses_full_dps;
    let dmg = |s: &pob::Summary| damage(full, s);
    let label = if full { "Full DPS" } else { "DPS" };

    println!("{}", plan.build.describe());
    println!(
        "PoB2 {}, {} engines, {} additions and {} removals screened per step",
        plan.build.pob_version,
        pool.size(),
        plan.screened.0,
        plan.screened.1
    );
    println!(
        "Scored on your realistic build ({} of {} conditions kept): {} {label}, EHP {}, smallest max hit {}",
        plan.kept_conditions,
        plan.total_conditions,
        format_number(dmg(&plan.start)),
        format_number(plan.start.ehp),
        format_number(plan.start.min_hit),
    );
    println!();

    if plan.steps.is_empty() {
        println!(
            "No point-neutral passive swap within {} points improves this goal by {}% or more.",
            options.max_distance,
            analysis::plan::MIN_GAIN
        );
    } else {
        let goal = if options.goal == Goal::Damage {
            "damage"
        } else {
            "survival"
        };
        println!(
            "Passive tree plan for {goal}: each step builds on the one before (respec costs gold only)"
        );
        for (i, step) in plan.steps.iter().enumerate() {
            let (b, a) = (&step.before, &step.after);
            println!();
            println!(
                "{:>3}.  {} {label}   {} EHP   {} smallest max hit",
                i + 1,
                signed(pct(dmg(b), dmg(a))),
                signed(pct(b.ehp, a.ehp)),
                signed(pct(b.min_hit, a.min_hit)),
            );
            print_wrapped("Take", &describe_nodes(&step.take));
            print_wrapped("Drop", &describe_nodes(&step.drop));
        }
        let end = &plan.steps.last().expect("non-empty").after;
        println!();
        println!(
            "All {} steps: {} → {} {label} ({}), EHP {}, smallest max hit {}",
            plan.steps.len(),
            format_number(dmg(&plan.start)),
            format_number(dmg(end)),
            signed(pct(dmg(&plan.start), dmg(end))),
            signed(pct(plan.start.ehp, end.ehp)),
            signed(pct(plan.start.min_hit, end.min_hit)),
        );
        if plan.kept_conditions < plan.total_conditions {
            println!(
                "Judged without the {} conditions you did not keep: a node that needs one (e.g. an",
                plan.total_conditions - plan.kept_conditions
            );
            println!(
                "enemy Shocked) counts as worth nothing. Keep what you really sustain with --keep."
            );
        }
        println!();
        println!(
            "Every step was applied to the tree and fully recalculated by PoB, then the result"
        );
        println!("reloaded fresh. Same point count, no new PoB warnings.");
        match export {
            Some(path) => {
                fs::write(&path, &plan.code)?;
                println!(
                    "PoB code of the final tree written to {} (import it in PoB2 to check).",
                    path.display()
                );
            }
            None => println!(
                "Add --export <file> to get the final tree as a PoB code to check in PoB2."
            ),
        }
    }
    if !plan.rejected_warnings.is_empty() {
        println!(
            "Skipped after PoB's exact check: {}",
            plan.rejected_warnings.join("; ")
        );
    }
    println!(
        "New path attribute nodes take PoB's default attribute; choose what you need in game."
    );
    Ok(ExitCode::SUCCESS)
}

/// Notables and keystones by name, small nodes by their stats:
/// "Harbinger of Disaster; 2× +8 to Intelligence".
pub fn describe_nodes(nodes: &[NodeRef]) -> String {
    let mut named: Vec<String> = Vec::new();
    let mut small: Vec<(String, usize)> = Vec::new();
    for node in nodes {
        if node.kind == "Notable" || node.kind == "Keystone" {
            named.push(node.name.clone());
        } else {
            let text = if node.stats.is_empty() {
                node.name.clone()
            } else {
                node.stats.join(", ")
            };
            match small.iter_mut().find(|(t, _)| *t == text) {
                Some((_, count)) => *count += 1,
                None => small.push((text, 1)),
            }
        }
    }
    named.sort();
    let small = small.into_iter().map(|(text, count)| {
        if count == 1 {
            text
        } else {
            format!("{count}× {text}")
        }
    });
    let parts: Vec<String> = named.into_iter().chain(small).collect();
    if parts.is_empty() {
        "nothing".into()
    } else {
        parts.join("; ")
    }
}

/// "      Take text…" wrapped at 100 columns with a hanging indent.
fn print_wrapped(label: &str, text: &str) {
    const WIDTH: usize = 100;
    let mut line = format!("      {label:<4} ");
    for word in text.split(' ') {
        if line.chars().count() + word.chars().count() + 1 > WIDTH
            && line.trim().len() > label.len()
        {
            println!("{}", line.trim_end());
            line = " ".repeat(11);
        }
        line.push_str(word);
        line.push(' ');
    }
    println!("{}", line.trim_end());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_nodes_plainly() {
        let n = |kind: &str, name: &str, stat: &str| NodeRef {
            id: 0,
            name: name.into(),
            kind: kind.into(),
            stats: if stat.is_empty() {
                vec![]
            } else {
                vec![stat.into()]
            },
        };
        assert_eq!(
            describe_nodes(&[
                n("Notable", "B", "x"),
                n("Normal", "s", "+8 to Intelligence"),
                n("Keystone", "A", "y"),
                n("Normal", "s", "+8 to Intelligence"),
            ]),
            "A; B; 2× +8 to Intelligence"
        );
        assert_eq!(describe_nodes(&[n("Normal", "Small", "")]), "Small");
    }
}
