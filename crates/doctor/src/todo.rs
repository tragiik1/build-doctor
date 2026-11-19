//! `doctor todo`: the ranked to-do list. This version covers the passive
//! tree, where changes cost only gold.
//!
//! The result is a plan: the best point-neutral swap, applied, then the best
//! swap on the improved tree, and so on, each step confirmed by PoB. A swap
//! allocates a node (with its path) and frees the same number of points
//! elsewhere. Point-neutral matters because PoB derives the character level
//! from points spent in its auto-level mode, so adding or removing points
//! alone would quietly change the level.
//!
//! The objective is the build's realistic numbers: the build is loaded with
//! every combat condition it assumes removed from its configuration, except
//! the ones the player keeps (`--keep`, as in `doctor realistic`).
//!
//! Each step: screen every option with PoB's fast node calculator, pair
//! promising additions with the cheapest point-equal set of removals, screen
//! the pairs together, then apply the best pairs to the tree exactly as
//! clicking in PoB would and let PoB recalculate fully. A step is taken only
//! if PoB's exact result improves the goal, spends no extra points and raises
//! no new PoB warning (such as an unmet attribute requirement). The improved
//! build is then reloaded fresh, as importing it into PoB would, and every
//! engine must reproduce those numbers before the next step.

use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use pob::{Install, NodeRef, Pool, StatSheet, Summary, TreeOption, TreeScreen, TreeTrial};

use crate::AnyResult;
use crate::check::{describe_build, format_number};
use crate::conditions::{self, Keep};

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Goal {
    /// More damage; survival changes are shown alongside.
    Damage,
    /// A larger smallest max hit; total EHP breaks ties.
    Survival,
}

#[derive(Debug, Clone)]
pub struct Options {
    pub keep: Vec<String>,
    pub goal: Goal,
    pub jobs: usize,
    pub max_distance: u32,
    pub steps: usize,
    pub export: Option<PathBuf>,
}

/// How many screened additions and removals the pairing considers, and how
/// many pairs get an exact trial per step.
const ADD_POOL: usize = 32;
const REMOVE_POOL: usize = 24;
const TRIALS: usize = 8;

/// The goal's number in a summary. Damage is Full DPS when the build uses
/// it, otherwise the main skill's and minions' combined DPS.
fn goal_value(goal: Goal, uses_full_dps: bool, s: &Summary) -> f64 {
    match goal {
        Goal::Damage if uses_full_dps => s.full_dps,
        Goal::Damage => s.combined_dps + s.minion_dps,
        Goal::Survival => s.min_hit,
    }
}

fn damage(uses_full_dps: bool, s: &Summary) -> f64 {
    goal_value(Goal::Damage, uses_full_dps, s)
}

fn pct(before: f64, after: f64) -> f64 {
    if before == 0.0 {
        0.0
    } else {
        (after - before) / before * 100.0
    }
}

/// A screened swap: allocate `add`, free the points of `removes`.
#[derive(Debug, Clone)]
struct Pair<'a> {
    add: &'a TreeOption,
    removes: Vec<&'a TreeOption>,
    screened_gain: f64,
}

/// The cheapest set of removal options freeing exactly `points`, with no
/// node in common with each other or with `avoid`. Cost is the sum of each
/// option's screened loss, an estimate the pair screen then checks.
fn cheapest_removals<'a>(
    options: &[(&'a TreeOption, f64)],
    points: u32,
    avoid: &HashSet<u32>,
) -> Option<Vec<&'a TreeOption>> {
    fn search<'a>(
        options: &[(&'a TreeOption, f64)],
        start: usize,
        remaining: u32,
        used: &mut HashSet<u32>,
        chosen: &mut Vec<&'a TreeOption>,
        cost: f64,
        best: &mut Option<(f64, Vec<&'a TreeOption>)>,
    ) {
        if remaining == 0 {
            if best.as_ref().is_none_or(|(b, _)| cost < *b) {
                *best = Some((cost, chosen.clone()));
            }
            return;
        }
        if best.as_ref().is_some_and(|(b, _)| cost >= *b) {
            return;
        }
        for i in start..options.len() {
            let (option, loss) = options[i];
            if option.cost == 0
                || option.cost > remaining
                || option.nodes.iter().any(|n| used.contains(n))
            {
                continue;
            }
            used.extend(option.nodes.iter().copied());
            chosen.push(option);
            search(
                options,
                i + 1,
                remaining - option.cost,
                used,
                chosen,
                cost + loss.max(0.0),
                best,
            );
            chosen.pop();
            for n in &option.nodes {
                used.remove(n);
            }
        }
    }
    let mut best = None;
    let mut used = avoid.clone();
    search(
        options,
        0,
        points,
        &mut used,
        &mut Vec::new(),
        0.0,
        &mut best,
    );
    best.map(|(_, set)| set)
}

/// A step of the plan, with PoB's numbers before and after it.
struct Step {
    trial: TreeTrial,
    before: Summary,
    after: Summary,
}

/// A build state the plan has reached: PoB's saved output and its stats.
struct State {
    xml: String,
    stats: StatSheet,
}

/// Smallest exact improvement worth a step, in percent of the goal.
const MIN_GAIN: f64 = 0.1;

pub fn run(install: &Install, xml: &str, options: &Options) -> AnyResult<ExitCode> {
    let started = Instant::now();
    let pool = Pool::new(install, options.jobs)?;
    let original = conditions::inspect(&pool, xml)?;
    let conditions = conditions::conditions(&original.assumptions);
    let keep = Keep::new(&options.keep, &conditions)?;
    let dropped: Vec<&str> = conditions
        .iter()
        .filter(|c| !keep.keeps(&c.label))
        .map(|c| c.var.as_str())
        .collect();
    let kept_count = conditions.len() - dropped.len();

    // The realistic build, as PoB saves it after loading.
    let realistic = conditions::inspect(&pool, &pob::config_xml::without_inputs(xml, &dropped)?)?;
    let start = State {
        xml: realistic.calc.xml,
        stats: realistic.calc.stats,
    };
    let start_summary = Summary::from_stats(&start.stats);
    let goal = options.goal;
    let uses_full_dps = start_summary.full_dps > 0.0;
    if goal_value(goal, uses_full_dps, &start_summary) <= 0.0 {
        return Err("PoB reports no value for this goal on the realistic build".into());
    }

    let mut steps: Vec<Step> = Vec::new();
    let mut current = start;
    let mut screened = (0, 0);
    let mut rejected: Vec<String> = Vec::new();
    while steps.len() < options.steps {
        let before = Summary::from_stats(&current.stats);
        let Some(trial) = best_swap(
            &pool,
            &current,
            goal,
            uses_full_dps,
            options.max_distance,
            &mut screened,
            &mut rejected,
        )?
        else {
            break;
        };
        // Reload the improved build fresh, as importing it into PoB would.
        let fresh = conditions::inspect(&pool, &trial.calc.xml)?;
        let after = Summary::from_stats(&fresh.calc.stats);
        if pct(
            goal_value(goal, uses_full_dps, &before),
            goal_value(goal, uses_full_dps, &after),
        ) < MIN_GAIN
        {
            break;
        }
        current = State {
            xml: fresh.calc.xml,
            stats: fresh.calc.stats,
        };
        steps.push(Step {
            trial,
            before,
            after,
        });
    }

    // Report.
    println!("{}", describe_build(&original.id));
    println!(
        "PoB2 {}, {} engines, {} additions and {} removals screened per step, {:.1}s",
        install.version(),
        pool.size(),
        screened.0,
        screened.1,
        started.elapsed().as_secs_f64()
    );
    let damage_label = if uses_full_dps { "Full DPS" } else { "DPS" };
    println!(
        "Scored on your realistic build ({} of {} conditions kept{}): {} {damage_label}, EHP {}, smallest max hit {}",
        kept_count,
        conditions.len(),
        if kept_count == 0 && !conditions.is_empty() {
            "; keep what you sustain with --keep"
        } else {
            ""
        },
        format_number(damage(uses_full_dps, &start_summary)),
        format_number(start_summary.ehp),
        format_number(start_summary.min_hit),
    );
    println!();

    if steps.is_empty() {
        println!(
            "No point-neutral passive swap within {} points improves this goal by {MIN_GAIN}% or more.",
            options.max_distance
        );
    } else {
        println!("Passive tree plan: each step builds on the one before (respec costs gold only)");
        for (i, step) in steps.iter().enumerate() {
            let (b, a) = (&step.before, &step.after);
            println!();
            println!(
                "{:>3}.  {} {damage_label}   {} EHP   {} smallest max hit",
                i + 1,
                signed(pct(damage(uses_full_dps, b), damage(uses_full_dps, a))),
                signed(pct(b.ehp, a.ehp)),
                signed(pct(b.min_hit, a.min_hit)),
            );
            print_wrapped("Take", &describe_nodes(&step.trial.added));
            print_wrapped("Drop", &describe_nodes(&step.trial.removed));
        }
        let end = &steps.last().expect("non-empty").after;
        println!();
        println!(
            "All {} steps: {} → {} {damage_label} ({}), EHP {}, smallest max hit {}",
            steps.len(),
            format_number(damage(uses_full_dps, &start_summary)),
            format_number(damage(uses_full_dps, end)),
            signed(pct(
                damage(uses_full_dps, &start_summary),
                damage(uses_full_dps, end)
            )),
            signed(pct(start_summary.ehp, end.ehp)),
            signed(pct(start_summary.min_hit, end.min_hit)),
        );
        if kept_count < conditions.len() {
            println!(
                "Judged without the {} conditions you did not keep: a node that needs one (e.g. an",
                conditions.len() - kept_count
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
        match &options.export {
            Some(path) => {
                fs::write(path, pob::code::encode(&current.xml))?;
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
    if !rejected.is_empty() {
        rejected.sort();
        rejected.dedup();
        println!("Skipped after PoB's exact check: {}", rejected.join("; "));
    }
    println!(
        "New path attribute nodes take PoB's default attribute; choose what you need in game."
    );
    Ok(ExitCode::SUCCESS)
}

/// The best exact swap on `current`, if any improves the goal.
fn best_swap(
    pool: &Pool,
    current: &State,
    goal: Goal,
    uses_full_dps: bool,
    max_distance: u32,
    screened: &mut (usize, usize),
    rejected: &mut Vec<String>,
) -> AnyResult<Option<TreeTrial>> {
    // Screen, split over the pool.
    let parts = pool.size() * 2;
    let pieces = pool.map(
        &current.xml,
        Some(&current.stats),
        (0..parts).collect(),
        move |e, part| e.tree_screen(max_distance, part, parts),
    )?;
    let screen = merge(pieces);
    *screened = (screen.adds.len(), screen.removes.len());
    let base = goal_value(goal, uses_full_dps, &screen.base);
    let gain = |s: &Summary| pct(base, goal_value(goal, uses_full_dps, s));

    let mut adds: Vec<(&TreeOption, f64)> = screen
        .adds
        .iter()
        .map(|o| (o, gain(&o.screened)))
        .filter(|(_, g)| *g > 0.0)
        .collect();
    adds.sort_by(|a, b| (b.1 / f64::from(b.0.cost)).total_cmp(&(a.1 / f64::from(a.0.cost))));
    adds.truncate(ADD_POOL);
    let mut removes: Vec<(&TreeOption, f64)> = screen
        .removes
        .iter()
        .filter(|o| o.cost > 0)
        .map(|o| (o, -gain(&o.screened)))
        .collect();
    removes.sort_by(|a, b| (a.1 / f64::from(a.0.cost)).total_cmp(&(b.1 / f64::from(b.0.cost))));
    removes.truncate(REMOVE_POOL);

    // Pair each addition with the cheapest point-equal removals.
    let mut pairs: Vec<Pair> = Vec::new();
    for &(add, _) in &adds {
        let avoid: HashSet<u32> = add.nodes.iter().copied().collect();
        if let Some(set) = cheapest_removals(&removes, add.cost, &avoid) {
            pairs.push(Pair {
                add,
                removes: set,
                screened_gain: 0.0,
            });
        }
    }
    let combos: Vec<(Vec<u32>, Vec<u32>)> = pairs
        .iter()
        .map(|p| {
            (
                p.removes
                    .iter()
                    .flat_map(|o| o.nodes.iter().copied())
                    .collect(),
                p.add.nodes.clone(),
            )
        })
        .collect();
    let combined = pool.map(
        &current.xml,
        Some(&current.stats),
        combos,
        |e, (remove, add)| e.tree_screen_change(&remove, &add),
    )?;
    for (pair, summary) in pairs.iter_mut().zip(&combined) {
        pair.screened_gain = gain(summary);
    }
    pairs.retain(|p| p.screened_gain > 0.0);
    pairs.sort_by(|a, b| b.screened_gain.total_cmp(&a.screened_gain));
    pairs.truncate(TRIALS.max(pool.size()));

    // Exact trials.
    let jobs: Vec<(Vec<u32>, Vec<u32>)> = pairs
        .iter()
        .map(|p| {
            (
                p.removes.iter().map(|o| o.node.id).collect(),
                vec![p.add.node.id],
            )
        })
        .collect();
    let trials = pool.map(
        &current.xml,
        Some(&current.stats),
        jobs,
        |e, (remove, add)| e.try_tree(&remove, &add),
    )?;
    let exact_base = Summary::from_stats(&current.stats);
    let mut best: Option<(f64, TreeTrial)> = None;
    for trial in trials {
        if trial.points_after != trial.points_before {
            continue; // a removal changed the path PoB takes to the node
        }
        if !trial.new_warnings.is_empty() {
            rejected.extend(trial.new_warnings.iter().cloned());
            continue;
        }
        let g = pct(
            goal_value(goal, uses_full_dps, &exact_base),
            goal_value(goal, uses_full_dps, &Summary::from_stats(&trial.calc.stats)),
        );
        if g >= MIN_GAIN && best.as_ref().is_none_or(|(b, _)| g > *b) {
            best = Some((g, trial));
        }
    }
    Ok(best.map(|(_, t)| t))
}

fn merge(pieces: Vec<TreeScreen>) -> TreeScreen {
    let mut merged = TreeScreen {
        base: pieces.first().map(|p| p.base).unwrap_or_default(),
        adds: Vec::new(),
        removes: Vec::new(),
    };
    for piece in pieces {
        merged.adds.extend(piece.adds);
        merged.removes.extend(piece.removes);
    }
    merged
}

/// Notables and keystones by name, small nodes by their stats:
/// "Harbinger of Disaster; 2× +8 to Intelligence".
fn describe_nodes(nodes: &[NodeRef]) -> String {
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

/// "     Take  text…" wrapped at 100 columns with a hanging indent.
fn print_wrapped(label: &str, text: &str) {
    const WIDTH: usize = 100;
    let indent = 11;
    let mut line = format!("      {label:<4} ");
    for word in text.split(' ') {
        if line.chars().count() + word.chars().count() + 1 > WIDTH
            && line.trim().len() > label.len()
        {
            println!("{}", line.trim_end());
            line = " ".repeat(indent);
        }
        line.push_str(word);
        line.push(' ');
    }
    println!("{}", line.trim_end());
}

fn signed(value: f64) -> String {
    if value.abs() < 0.05 {
        "0%".into()
    } else {
        format!("{value:+.1}%")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn option(id: u32, nodes: &[u32]) -> TreeOption {
        TreeOption {
            node: NodeRef {
                id,
                name: format!("n{id}"),
                kind: "Normal".into(),
                stats: vec![],
            },
            nodes: nodes.to_vec(),
            cost: nodes.len() as u32,
            screened: Summary::default(),
        }
    }

    #[test]
    fn cheapest_removals_matches_points_exactly() {
        let a = option(1, &[1]);
        let b = option(2, &[2, 3]);
        let c = option(4, &[4]);
        let opts = [(&a, 0.5), (&b, 0.2), (&c, 0.1)];
        let set = cheapest_removals(&opts, 2, &HashSet::new()).unwrap();
        let ids: Vec<u32> = set.iter().map(|o| o.node.id).collect();
        assert_eq!(
            ids,
            [2],
            "one 2-point removal at 0.2 beats two 1-pointers at 0.6"
        );
        assert!(cheapest_removals(&opts, 5, &HashSet::new()).is_none());
    }

    #[test]
    fn cheapest_removals_never_overlaps_or_touches_the_path() {
        let a = option(1, &[1, 2]);
        let b = option(2, &[2]);
        let c = option(3, &[3]);
        let opts = [(&a, 0.0), (&b, 0.0), (&c, 0.0)];
        let set = cheapest_removals(&opts, 3, &HashSet::new()).unwrap();
        let mut nodes: Vec<u32> = set.iter().flat_map(|o| o.nodes.clone()).collect();
        nodes.sort_unstable();
        assert_eq!(nodes, [1, 2, 3]);
        let avoid: HashSet<u32> = [3].into_iter().collect();
        assert!(cheapest_removals(&opts, 3, &avoid).is_none());
    }

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
