//! A step-by-step passive tree plan, where changes cost only gold.
//!
//! Each step is the best point-neutral swap found: allocate a node (with its
//! path) and free the same number of points elsewhere. Point-neutral matters
//! because PoB derives the character level from points spent in its
//! auto-level mode, so adding or removing points alone would quietly change
//! the level.
//!
//! The objective is the build's realistic numbers: the build is loaded with
//! every combat condition removed from its configuration, except the ones
//! the player keeps.
//!
//! Each step: screen every option with PoB's fast node calculator (split
//! over the pool), pair promising additions with the cheapest point-equal
//! set of removals, screen the pairs together, then apply the best pairs to
//! the tree exactly as clicking in PoB would and let PoB recalculate fully.
//! A step is taken only if PoB's exact result improves the goal by at least
//! [`MIN_GAIN`] percent, spends no extra points and raises no new PoB warning
//! (such as an unmet attribute requirement). The improved build is then
//! reloaded fresh, as importing it into PoB would, and every engine must
//! reproduce those numbers before the next step.

use std::collections::HashSet;

use pob::{NodeRef, Pool, StatSheet, Summary, TreeOption, TreeScreen, TreeTrial};
use serde::Serialize;

use crate::{BuildInfo, Error, Result, check_kept, conditions, damage, inspect, pct};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Goal {
    /// More damage; survival changes are reported alongside.
    Damage,
    /// A larger smallest max hit.
    Survival,
}

#[derive(Debug, Clone)]
pub struct PlanOptions {
    /// PoB config variables of conditions the player keeps up.
    pub kept: Vec<String>,
    pub goal: Goal,
    pub max_distance: u32,
    pub steps: usize,
}

impl Default for PlanOptions {
    fn default() -> Self {
        Self {
            kept: Vec::new(),
            goal: Goal::Damage,
            max_distance: 6,
            steps: 5,
        }
    }
}

/// Where a running plan is.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanProgress {
    /// 1-based step being searched.
    pub step: usize,
    pub steps: usize,
    pub phase: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub build: BuildInfo,
    pub goal: Goal,
    pub uses_full_dps: bool,
    pub kept_conditions: usize,
    pub total_conditions: usize,
    pub start: Summary,
    pub steps: Vec<PlanStep>,
    /// Options screened per step (additions, removals).
    pub screened: (usize, usize),
    /// PoB warnings that made it reject otherwise better swaps.
    pub rejected_warnings: Vec<String>,
    /// The final build as a PoB code (the start build when there are no steps).
    pub code: String,
    /// True when the plan stopped because it was cancelled.
    pub cancelled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
    pub take: Vec<NodeRef>,
    pub drop: Vec<NodeRef>,
    pub before: Summary,
    pub after: Summary,
}

/// Smallest exact improvement worth a step, in percent of the goal.
pub const MIN_GAIN: f64 = 0.1;

/// How many screened additions and removals the pairing considers, and how
/// many pairs get an exact trial per step.
const ADD_POOL: usize = 32;
const REMOVE_POOL: usize = 24;
const TRIALS: usize = 8;

fn goal_value(goal: Goal, uses_full_dps: bool, s: &Summary) -> f64 {
    match goal {
        Goal::Damage => damage(uses_full_dps, s),
        Goal::Survival => s.min_hit,
    }
}

/// A build state the plan has reached: PoB's saved output and its stats.
struct State {
    xml: String,
    stats: StatSheet,
}

/// Builds the plan. `progress` is told what is happening; `cancelled` is
/// polled between phases and stops the plan after the current one.
pub fn plan(
    pool: &Pool,
    xml: &str,
    options: &PlanOptions,
    progress: &dyn Fn(PlanProgress),
    cancelled: &dyn Fn() -> bool,
) -> Result<Plan> {
    let report = |step: usize, phase: &'static str| {
        progress(PlanProgress {
            step,
            steps: options.steps,
            phase,
        })
    };
    report(1, "Loading your realistic build");
    let original = inspect(pool, xml)?;
    let conditions = conditions(&original.assumptions);
    check_kept(&conditions, &options.kept)?;
    let dropped: Vec<&str> = conditions
        .iter()
        .filter(|c| !options.kept.contains(&c.var))
        .map(|c| c.var.as_str())
        .collect();

    let realistic = inspect(pool, &pob::config_xml::without_inputs(xml, &dropped)?)?;
    let mut current = State {
        xml: realistic.xml,
        stats: realistic.stats,
    };
    let start = Summary::from_stats(&current.stats);
    let goal = options.goal;
    let uses_full_dps = start.full_dps > 0.0;
    if goal_value(goal, uses_full_dps, &start) <= 0.0 {
        return Err(Error::Analysis(
            "PoB reports no value for this goal on the realistic build".into(),
        ));
    }

    let mut steps: Vec<PlanStep> = Vec::new();
    let mut screened = (0, 0);
    let mut rejected: Vec<String> = Vec::new();
    let mut was_cancelled = false;
    while steps.len() < options.steps {
        if cancelled() {
            was_cancelled = true;
            break;
        }
        let step = steps.len() + 1;
        let before = Summary::from_stats(&current.stats);
        let search = Search {
            pool,
            goal,
            uses_full_dps,
            max_distance: options.max_distance,
        };
        let Some(trial) = search.best_swap(
            &current,
            &mut screened,
            &mut rejected,
            &|phase| report(step, phase),
            cancelled,
        )?
        else {
            was_cancelled = cancelled();
            break;
        };
        // Reload the improved build fresh, as importing it into PoB would.
        report(step, "Reloading the improved build");
        let fresh = inspect(pool, &trial.calc.xml)?;
        let after = Summary::from_stats(&fresh.stats);
        if pct(
            goal_value(goal, uses_full_dps, &before),
            goal_value(goal, uses_full_dps, &after),
        ) < MIN_GAIN
        {
            break;
        }
        current = State {
            xml: fresh.xml,
            stats: fresh.stats,
        };
        steps.push(PlanStep {
            take: trial.added,
            drop: trial.removed,
            before,
            after,
        });
    }
    rejected.sort();
    rejected.dedup();

    Ok(Plan {
        build: BuildInfo::new(&original.id, &pool.install().version().to_string()),
        goal,
        uses_full_dps,
        kept_conditions: conditions.len() - dropped.len(),
        total_conditions: conditions.len(),
        start,
        steps,
        screened,
        rejected_warnings: rejected,
        code: pob::code::encode(&current.xml),
        cancelled: was_cancelled,
    })
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

struct Search<'p> {
    pool: &'p Pool,
    goal: Goal,
    uses_full_dps: bool,
    max_distance: u32,
}

impl Search<'_> {
    /// The best exact swap on `current`, if any improves the goal.
    fn best_swap(
        &self,
        current: &State,
        screened: &mut (usize, usize),
        rejected: &mut Vec<String>,
        phase: &dyn Fn(&'static str),
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Option<TreeTrial>> {
        let Self {
            pool,
            goal,
            uses_full_dps,
            max_distance,
        } = *self;
        phase("Screening every passive within reach");
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
        if cancelled() {
            return Ok(None);
        }
        phase("Screening the best swaps together");
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

        if cancelled() {
            return Ok(None);
        }
        phase("Trying the best swaps exactly");
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

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
}
