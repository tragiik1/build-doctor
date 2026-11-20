//! What is wrong with a build, most important first.
//!
//! Every finding is a number PoB calculates, and every gain is PoB
//! recalculating the build with the fix applied as a line in its Custom
//! Modifiers box (for example "+49% to Lightning Resistance"), so a fix is
//! worth exactly what PoB says it is. A fix PoB does not confirm (the
//! resistance does not reach its maximum, hit chance does not reach 100%) is
//! reported without a gain, and a fix that changes none of PoB's numbers is
//! not reported.

use std::collections::HashMap;

use pob::{Outputs, Pool, SkillUse, StatKind, StatSheet};
use serde::Serialize;

use crate::{BuildInfo, Result, pct};

/// Most accuracy the search adds before giving up.
const ACCURACY_LIMIT: u32 = 25_600;
const RESISTANCES: &[&str] = &["Fire", "Cold", "Lightning", "Chaos"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Severity {
    /// The numbers are off, or include something the game would not allow.
    Problem,
    /// A fix PoB measured.
    Gain,
    /// Worth knowing; no number changes.
    Note,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Finding {
    pub severity: Severity,
    pub title: String,
    pub detail: Option<String>,
    /// What fixes it, as the modifier line PoB evaluated.
    pub fix: Option<String>,
    /// What the fix changes, as PoB calculated it.
    pub effects: Vec<Effect>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Effect {
    pub label: String,
    pub before: f64,
    pub after: f64,
    /// Percent change.
    pub change: f64,
    /// The values are percentages themselves (a resistance), so show them
    /// as "75% → 56%" rather than with a percent change.
    pub percent: bool,
}

impl Effect {
    pub(crate) fn new(label: impl Into<String>, before: f64, after: f64) -> Self {
        Self {
            label: label.into(),
            before,
            after,
            change: pct(before, after),
            percent: false,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnosis {
    pub build: BuildInfo,
    pub findings: Vec<Finding>,
    /// What one more level of each kind of skill does to the build's
    /// damage, best first; only kinds that change it.
    pub levels: Vec<LevelWorth>,
}

/// "+1 to Level of all Spell Skills" and what PoB says it does.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LevelWorth {
    pub line: String,
    pub damage: Effect,
    /// PoB warnings the extra level adds (a gem's attribute requirement
    /// the build no longer meets, say), which explain a loss.
    pub new_warnings: Vec<String>,
}

/// One PoB recalculation (or search) to run in the pool.
enum Job {
    /// Raise a resistance by `raise` (at most `missing`, the gap to its
    /// maximum).
    Resist {
        element: &'static str,
        now: f64,
        missing: f64,
        raise: f64,
    },
    /// One more level of a kind of skill.
    Level(String),
    /// The build without its resistance-penalty setting (PoB's default is
    /// the endgame penalty).
    Penalty,
    /// A quick probe: does this much more flat accuracy bring hit chance
    /// to 100%?
    Accuracy(u32),
}

enum Outcome {
    Resist {
        element: &'static str,
        now: f64,
        missing: f64,
        raise: f64,
        reached: bool,
        stats: StatSheet,
    },
    Level {
        line: String,
        stats: StatSheet,
        warnings: Vec<String>,
    },
    Penalty(StatSheet),
    Accuracy {
        added: u32,
        hits: bool,
    },
}

/// What the accuracy search found.
enum AccuracyAnswer {
    /// This much flat accuracy brings hit chance to 100% (and 1% less does
    /// not), with the build's stats at it; both confirmed by full,
    /// settled recalculations.
    Needs(u32, StatSheet),
    /// Even `ACCURACY_LIMIT` does not.
    Unreachable,
    /// The full recalculation disagreed with the quick probes.
    Unconfirmed,
}

fn output_keys() -> Vec<String> {
    let mut keys = vec![
        "AccuracyHitChance".to_owned(),
        "Spirit".to_owned(),
        "SpiritUnreserved".to_owned(),
    ];
    for element in RESISTANCES {
        keys.push(format!("{element}Resist"));
        keys.push(format!("Missing{element}Resist"));
        keys.push(format!("{element}ResistOverCap"));
    }
    keys
}

pub fn diagnose(pool: &Pool, xml: &str) -> Result<Diagnosis> {
    let mut found = pool.map(xml, None, vec![output_keys()], |engine, keys| {
        let keys: Vec<&str> = keys.iter().map(String::as_str).collect();
        Ok(Read {
            id: engine.identity()?,
            assumptions: engine.assumptions()?,
            warnings: engine.warnings()?,
            unsupported: engine.unsupported_mods()?.len(),
            out: engine.outputs(&keys)?,
            cheapest_spirit: engine.cheapest_spirit()?,
            skills: engine.skill_usage()?,
            chaos_roll: engine.roll_range("#% to Chaos Resistance")?,
            level_lines: engine.level_lines()?,
            saved: engine.ensure_original()?,
        })
    })?;
    let read = found.pop().expect("one item in, one out");
    let base = StatSheet::from_build_xml(&read.saved)?;
    let out = &read.out;
    let value = |key: &str| out.get(key).copied();

    let mut jobs = Vec::new();
    if read
        .assumptions
        .iter()
        .any(|a| a.var == "resistancePenalty")
    {
        jobs.push(Job::Penalty);
    }
    for element in RESISTANCES {
        if let Some(missing) = value(&format!("Missing{element}Resist")).filter(|m| *m > 0.0) {
            jobs.push(Job::Resist {
                element,
                now: value(&format!("{element}Resist")).unwrap_or(0.0),
                missing,
                raise: resist_step(element, missing, read.chaos_roll.as_ref()),
            });
        }
    }
    jobs.extend(read.level_lines.iter().cloned().map(Job::Level));
    let misses = value("AccuracyHitChance").is_some_and(|h| h < 100.0);
    if misses {
        // The first round of the accuracy search: doubling steps.
        jobs.extend(
            std::iter::successors(Some(100), |n| (*n < ACCURACY_LIMIT).then(|| n * 2))
                .map(Job::Accuracy),
        );
    }

    let outcomes = pool.map(xml, Some(&base), jobs, |engine, job| match job {
        Job::Resist {
            element,
            now,
            missing,
            raise,
        } => {
            let key = format!("{element}Resist");
            let (calc, after) = engine.with_mods(&[&resist_line(element, raise)], &[&key])?;
            Ok(Outcome::Resist {
                element,
                now,
                missing,
                raise,
                reached: after.get(&key).is_some_and(|v| *v >= now + raise),
                stats: calc.stats,
            })
        }
        Job::Level(line) => {
            let (calc, _, warnings) = engine.with_mods_warned(&[&line], &[])?;
            Ok(Outcome::Level {
                line,
                stats: calc.stats,
                warnings,
            })
        }
        Job::Penalty => Ok(Outcome::Penalty(
            engine.without_config(&["resistancePenalty"])?.stats,
        )),
        Job::Accuracy(added) => probe_accuracy(engine, added),
    })?;

    let mut findings: Vec<Finding> = Vec::new();
    let mut levels: Vec<LevelWorth> = Vec::new();
    let mut search = AccuracySearch::default();
    for outcome in outcomes {
        findings.extend(match outcome {
            Outcome::Level {
                line,
                stats,
                warnings,
            } => {
                let (label, before, after) = damage_effect(&base, &stats);
                let damage = Effect::new(label, before, after);
                if before.is_finite() && after.is_finite() && damage.change.abs() >= 0.05 {
                    levels.push(LevelWorth {
                        line,
                        damage,
                        new_warnings: new_warnings(&read.warnings, &warnings, &read.skills),
                    });
                }
                None
            }
            Outcome::Penalty(stats) => penalty(&read.assumptions, &base, &stats),
            Outcome::Resist {
                element,
                now,
                missing,
                raise,
                reached,
                stats,
            } => resist(
                Raise {
                    element,
                    now,
                    missing,
                    raise,
                    reached,
                },
                read.chaos_roll.as_ref(),
                &base,
                &stats,
            ),
            Outcome::Accuracy { added, hits } => {
                search.record(added, hits);
                None
            }
        });
    }
    if misses {
        let answer = finish_accuracy(pool, xml, &base, search)?;
        findings.push(accuracy(out, &base, answer));
    }
    findings.extend(
        read.warnings
            .into_iter()
            .filter_map(|w| warning(w, &read.skills)),
    );
    findings.extend(over_cap(out));
    findings.extend(spirit(out, read.cheapest_spirit));
    if read.unsupported > 0 {
        findings.push(note(
            format!(
                "PoB ignores {} mod{} on your gear",
                read.unsupported,
                if read.unsupported == 1 { "" } else { "s" }
            ),
            "No number here includes them. Stats lists them.".into(),
        ));
    }
    rank(&mut findings);
    levels.sort_by(|a, b| {
        b.damage
            .change
            .total_cmp(&a.damage.change)
            .then_with(|| a.line.cmp(&b.line))
    });

    Ok(Diagnosis {
        build: BuildInfo::new(&read.id, &pool.install().version().to_string()),
        findings,
        levels,
    })
}

/// What the first pass reads from PoB.
struct Read {
    id: pob::Identity,
    assumptions: Vec<pob::Assumption>,
    warnings: Vec<String>,
    unsupported: usize,
    out: Outputs,
    cheapest_spirit: Option<f64>,
    skills: HashMap<String, SkillUse>,
    /// Lowest and highest explicit chaos resistance roll, and the category
    /// with the highest.
    chaos_roll: Option<(f64, f64, String)>,
    level_lines: Vec<String>,
    saved: String,
}

/// Problems first, then gains by their largest effect, then notes.
fn rank(findings: &mut [Finding]) {
    findings.sort_by(|a, b| {
        a.severity
            .cmp(&b.severity)
            .then_with(|| best_change(b).total_cmp(&best_change(a)))
    });
}

fn best_change(f: &Finding) -> f64 {
    f.effects.iter().map(|e| e.change).fold(0.0, f64::max)
}

fn note(title: String, detail: String) -> Finding {
    Finding {
        severity: Severity::Note,
        title,
        detail: Some(detail),
        fix: None,
        effects: Vec::new(),
    }
}

/// Effects of a change on player stats: (label, PoB stat). Only finite
/// values that changed are kept.
pub(crate) fn measured(
    stats: &[(String, String)],
    before: &StatSheet,
    after: &StatSheet,
) -> Vec<Effect> {
    stats
        .iter()
        .filter_map(|(label, key)| {
            let (b, a) = (before.player(key)?, after.player(key)?);
            (b.is_finite() && a.is_finite() && b != a).then(|| Effect {
                percent: key.ends_with("Resist") || key.ends_with("Chance"),
                ..Effect::new(label.clone(), b, a)
            })
        })
        .collect()
}

fn resist_line(element: &str, missing: f64) -> String {
    format!("+{}% to {element} Resistance", missing.ceil())
}

/// The middle of the best explicit chaos resistance roll in PoB's data:
/// one mod's worth. Few mods roll chaos resistance, so "raise it to the
/// maximum" is rarely within reach; one more mod is.
fn resist_step(element: &str, missing: f64, chaos_roll: Option<&(f64, f64, String)>) -> f64 {
    match (element, chaos_roll) {
        ("Chaos", Some((low, high, _))) => missing.min(((high - low) * 0.5 + low).ceil()),
        _ => missing,
    }
}

/// A resistance raise measured by PoB.
struct Raise<'a> {
    element: &'a str,
    now: f64,
    /// Gap to the maximum.
    missing: f64,
    /// What was added.
    raise: f64,
    /// Whether the resistance went up by `raise`.
    reached: bool,
}

/// A resistance below its maximum. Elemental resistances are raised to the
/// maximum; chaos resistance by one mod at a mid roll. Nothing when the
/// raise changes none of PoB's numbers (an immunity, for example).
fn resist(
    r: Raise<'_>,
    chaos_roll: Option<&(f64, f64, String)>,
    base: &StatSheet,
    fixed: &StatSheet,
) -> Option<Finding> {
    let element = r.element;
    let title = format!(
        "{element} resistance {}%, maximum {}%",
        fmt(r.now),
        fmt(r.now + r.missing)
    );
    if !r.reached {
        return Some(note(
            title,
            format!(
                "Adding {element} resistance does not raise it in PoB: something on the build sets it."
            ),
        ));
    }
    let effects = measured(
        &[
            (
                format!("{element} max hit"),
                format!("{element}MaximumHitTaken"),
            ),
            ("Total EHP".into(), "TotalEHP".into()),
        ],
        base,
        fixed,
    );
    let detail = match (element, chaos_roll) {
        ("Chaos", Some((low, high, _))) if r.raise < r.missing => Some(format!(
            "One chaos resistance mod at a mid roll (they roll +{}% to +{}% in PoB's data). Few mods roll chaos resistance, so the maximum is rarely within reach; this is what the next mod is worth.",
            fmt(*low),
            fmt(*high)
        )),
        _ => None,
    };
    (!effects.is_empty()).then(|| Finding {
        severity: Severity::Gain,
        title,
        detail,
        fix: Some(resist_line(element, r.raise)),
        effects,
    })
}

/// The build's resistance penalty, when it is not the endgame one PoB
/// defaults to (removing the setting changes PoB's numbers).
fn penalty(
    assumptions: &[pob::Assumption],
    base: &StatSheet,
    endgame: &StatSheet,
) -> Option<Finding> {
    let setting = assumptions.iter().find(|a| a.var == "resistancePenalty")?;
    let stats: Vec<(String, String)> = ["Fire", "Cold", "Lightning"]
        .iter()
        .map(|e| (format!("{e} resistance"), format!("{e}Resist")))
        .chain([("Total EHP".to_owned(), "TotalEHP".to_owned())])
        .collect();
    let effects = measured(&stats, base, endgame);
    (!effects.is_empty()).then(|| Finding {
        severity: Severity::Problem,
        title: format!("PoB uses the {} resistance penalty", setting.value),
        detail: Some(
            "Every number here assumes it. With the endgame penalty, PoB's default, your resistances are lower. The other findings use the build's setting."
                .into(),
        ),
        fix: None,
        effects,
    })
}

/// PoB warnings in `after` that are not in `before`, worded for the user.
/// For cost warnings ("You do not have enough Mana to use: A, B") only the
/// skills newly listed count, and only those the damage numbers include
/// (see [`warning`]): "Not enough Mana to use B".
pub(crate) fn new_warnings(
    before: &[String],
    after: &[String],
    skills: &HashMap<String, SkillUse>,
) -> Vec<String> {
    let listed = |need: &str, lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .filter_map(|l| l.split_once(" to use: "))
            .filter(|(n, _)| *n == need)
            .flat_map(|(_, list)| list.split(", ").map(str::to_owned))
            .collect()
    };
    let mut out = Vec::new();
    for line in after {
        if before.contains(line) {
            continue;
        }
        let Some((need, list)) = line.split_once(" to use: ") else {
            out.push(line.clone());
            continue;
        };
        let had = listed(need, before);
        let mut fresh: Vec<&str> = Vec::new();
        for name in list.split(", ") {
            let used = skills.get(name).is_none_or(|u| u.main || u.full_dps);
            if used && !had.iter().any(|h| h == name) && !fresh.contains(&name) {
                fresh.push(name);
            }
        }
        if !fresh.is_empty() {
            out.push(format!(
                "{} to use {}",
                need.replace("You do not have enough", "Not enough"),
                fresh.join(", ")
            ));
        }
    }
    out
}

/// A PoB warning, or nothing when it does not concern this build.
///
/// "You do not have enough Mana to use: A, B" matters for a skill the
/// damage numbers include (the main skill, or one counted in Full DPS). A
/// gem the player socketed but the numbers leave out only gets a note, and a
/// skill an item grants that the build does not use (a unique weapon's
/// expensive spell, say) is left out. Spare weapon-set points are a note.
/// Everything else PoB warns about is something the game would not allow.
fn warning(text: String, skills: &HashMap<String, SkillUse>) -> Option<Finding> {
    if text.contains("passives available") {
        return Some(note(
            text,
            "Unspent points on the second weapon set.".into(),
        ));
    }
    if let Some((need, list)) = text.split_once(" to use: ") {
        let mut names: Vec<&str> = Vec::new();
        for name in list.split(", ") {
            if !names.contains(&name) {
                names.push(name);
            }
        }
        // A name PoB lists but the skill groups do not (should not happen)
        // counts as used: better a warning too many than one missing.
        let used = |n: &&str| skills.get(*n).is_none_or(|u| u.main || u.full_dps);
        let socketed = |n: &&str| skills.get(*n).is_some_and(|u| u.socketed);
        let counted: Vec<&str> = names.iter().copied().filter(|n| used(n)).collect();
        let left_out: Vec<&str> = names
            .iter()
            .copied()
            .filter(|n| !used(n) && socketed(n))
            .collect();
        let need = need.replace("You do not have enough", "Not enough");
        if !counted.is_empty() {
            return Some(Finding {
                severity: Severity::Problem,
                title: format!("{need} to use {}", counted.join(", ")),
                detail: Some(
                    "Your damage numbers include it, but in game its cost is more than you have, so you could not use it."
                        .into(),
                ),
                fix: None,
                effects: Vec::new(),
            });
        }
        if !left_out.is_empty() {
            let detail = if left_out.len() == 1 {
                "It is not part of your damage numbers, but in game you could not use it."
            } else {
                "They are not part of your damage numbers, but in game you could not use them."
            };
            return Some(note(
                format!("{need} to use {}", left_out.join(", ")),
                detail.into(),
            ));
        }
        return None;
    }
    Some(Finding {
        severity: Severity::Problem,
        title: text,
        detail: Some(
            "PoB's own warning. The game would not allow this, so the numbers include something the character cannot have."
                .into(),
        ),
        fix: None,
        effects: Vec::new(),
    })
}

fn accuracy_line(added: u32) -> String {
    format!("+{added} to Accuracy Rating")
}

/// A quick probe (one recalculation): does `added` flat accuracy bring
/// PoB's hit chance to 100%?
fn probe_accuracy(engine: &pob::Engine, added: u32) -> pob::Result<Outcome> {
    let out = engine.probe_mods(&[&accuracy_line(added)], &["AccuracyHitChance"])?;
    Ok(Outcome::Accuracy {
        added,
        hits: reaches_100(&out),
    })
}

fn reaches_100(out: &Outputs) -> bool {
    out.get("AccuracyHitChance").is_some_and(|h| *h >= 100.0)
}

/// Runs the rest of the search with quick probes, then confirms the answer
/// with full, settled recalculations: the amount found must reach 100% and
/// the largest amount that missed must still miss.
fn finish_accuracy(
    pool: &Pool,
    xml: &str,
    base: &StatSheet,
    mut search: AccuracySearch,
) -> Result<AccuracyAnswer> {
    loop {
        let points = search.next(pool.size());
        if points.is_empty() {
            break;
        }
        for outcome in pool.map(xml, Some(base), points, probe_accuracy)? {
            if let Outcome::Accuracy { added, hits } = outcome {
                search.record(added, hits);
            }
        }
    }
    let Some(high) = search.high else {
        return Ok(AccuracyAnswer::Unreachable);
    };
    let mut checks = vec![high];
    if search.low > 0 {
        checks.push(search.low);
    }
    let mut confirmed = pool.map(xml, Some(base), checks, |engine, added| {
        let (calc, out) = engine.with_mods(&[&accuracy_line(added)], &["AccuracyHitChance"])?;
        Ok((reaches_100(&out), calc.stats))
    })?;
    let low_misses = confirmed.len() < 2 || !confirmed[1].0;
    let (high_hits, stats) = confirmed.swap_remove(0);
    Ok(if high_hits && low_misses {
        AccuracyAnswer::Needs(high, stats)
    } else {
        AccuracyAnswer::Unconfirmed
    })
}

/// The search for the least flat accuracy that brings hit chance to 100%.
/// Hit chance never falls as accuracy rises, so each round tests evenly
/// spaced amounts between the largest that misses and the smallest that
/// hits, one per engine, until the smallest that hits is within 1% of the
/// least possible.
#[derive(Default)]
struct AccuracySearch {
    /// Largest amount tried that misses.
    low: u32,
    /// Smallest amount tried that hits.
    high: Option<u32>,
}

impl AccuracySearch {
    fn record(&mut self, added: u32, hits: bool) {
        if !hits {
            self.low = self.low.max(added);
        } else if self.high.is_none_or(|h| added < h) {
            self.high = Some(added);
        }
    }

    /// Amounts to try next; none once the answer is within 1%.
    fn next(&self, engines: usize) -> Vec<u32> {
        let Some(high) = self.high else {
            return Vec::new();
        };
        let (low, high) = (u64::from(self.low), u64::from(high));
        if high <= low || high - low <= (high / 100).max(1) {
            return Vec::new();
        }
        let parts = engines.max(1) as u64 + 1;
        let mut points: Vec<u32> = (1..parts)
            .map(|i| (low + (high - low) * i / parts) as u32)
            .filter(|p| u64::from(*p) > low && u64::from(*p) < high)
            .collect();
        points.dedup();
        points
    }
}

fn accuracy(out: &Outputs, base: &StatSheet, answer: AccuracyAnswer) -> Finding {
    let chance = out.get("AccuracyHitChance").copied().unwrap_or(0.0);
    let title = format!("Hit chance {}%", fmt(chance));
    let (needed, stats) = match answer {
        AccuracyAnswer::Needs(needed, stats) => (needed, stats),
        AccuracyAnswer::Unreachable => {
            return note(
                title,
                format!(
                    "Even +{} accuracy does not reach 100% in PoB.",
                    group(ACCURACY_LIMIT.into())
                ),
            );
        }
        AccuracyAnswer::Unconfirmed => {
            return note(
                title,
                "PoB's full recalculation did not confirm how much accuracy reaches 100%, so no amount is given."
                    .into(),
            );
        }
    };
    let (label, before, after) = damage_effect(base, &stats);
    if !(before.is_finite() && after.is_finite() && after > before) {
        return note(
            title,
            format!(
                "+{} accuracy brings it to 100%, but PoB's damage does not change.",
                group(needed.into())
            ),
        );
    }
    Finding {
        severity: Severity::Gain,
        title,
        detail: Some(
            "Within 1% of the least that reaches 100%, against the enemy and distance set in PoB's configuration."
                .into(),
        ),
        fix: Some(accuracy_line(needed)),
        effects: vec![Effect::new(label, before, after)],
    }
}

/// The build's damage number: Full DPS when it uses it, otherwise the main
/// skill's and minions' combined DPS.
pub(crate) fn damage_effect(base: &StatSheet, after: &StatSheet) -> (&'static str, f64, f64) {
    let full = |s: &StatSheet| s.player("FullDPS").unwrap_or(0.0);
    if full(base) > 0.0 {
        return ("Full DPS", full(base), full(after));
    }
    let combined = |s: &StatSheet| {
        s.player("CombinedDPS").unwrap_or(0.0)
            + s.get(StatKind::Minion, "CombinedDPS")
                .and_then(pob::Stat::number)
                .unwrap_or(0.0)
    };
    ("DPS", combined(base), combined(after))
}

fn over_cap(out: &Outputs) -> Option<Finding> {
    let parts: Vec<String> = RESISTANCES
        .iter()
        .filter_map(|e| {
            let over = out.get(&format!("{e}ResistOverCap")).copied()?;
            (over > 0.0).then(|| format!("{} {}%", e.to_lowercase(), fmt(over)))
        })
        .collect();
    (!parts.is_empty()).then(|| {
        note(
            format!("Resistance over the maximum: {}", parts.join(", ")),
            "It only counts when an enemy lowers your resistances; otherwise those mods could be other stats."
                .into(),
        )
    })
}

/// Unreserved Spirit, when it is enough for the cheapest Spirit skill or
/// support in PoB's data.
fn spirit(out: &Outputs, cheapest: Option<f64>) -> Option<Finding> {
    let total = out.get("Spirit").copied().filter(|s| *s > 0.0)?;
    let cheapest = cheapest?;
    let free = out
        .get("SpiritUnreserved")
        .copied()
        .filter(|s| *s >= cheapest)?;
    Some(note(
        format!("{} of {} Spirit unreserved", fmt(free), fmt(total)),
        format!(
            "Enough for another Spirit skill or support; the cheapest in PoB's data reserves {}.",
            fmt(cheapest)
        ),
    ))
}

/// Whole numbers without decimals, others to one decimal place.
fn fmt(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn group(v: u64) -> String {
    let s = v.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn finding(severity: Severity, change: f64) -> Finding {
        Finding {
            severity,
            title: String::new(),
            detail: None,
            fix: None,
            effects: vec![Effect::new("x", 100.0, 100.0 + change)],
        }
    }

    #[test]
    fn ranks_problems_then_gains_by_size_then_notes() {
        let mut list = vec![
            finding(Severity::Note, 0.0),
            finding(Severity::Gain, 5.0),
            finding(Severity::Problem, 0.0),
            finding(Severity::Gain, 40.0),
        ];
        rank(&mut list);
        let order: Vec<_> = list.iter().map(|f| (f.severity, best_change(f))).collect();
        assert_eq!(
            order,
            vec![
                (Severity::Problem, 0.0),
                (Severity::Gain, 40.0),
                (Severity::Gain, 5.0),
                (Severity::Note, 0.0)
            ]
        );
    }

    #[test]
    fn spare_weapon_set_points_are_a_note() {
        let none = HashMap::new();
        let spare = warning("You have 7 Weapon set 2 passives available".into(), &none).unwrap();
        assert_eq!(spare.severity, Severity::Note);
        let spirit = warning("Your unreserved Spirit is negative".into(), &none).unwrap();
        assert_eq!(spirit.severity, Severity::Problem);
    }

    #[test]
    fn cost_warnings_only_for_skills_the_build_uses() {
        let skill = |main, full_dps, socketed| SkillUse {
            main,
            full_dps,
            socketed,
        };
        let skills: HashMap<String, SkillUse> = [
            ("Spark".to_owned(), skill(true, false, true)),
            ("The Stars Answer".to_owned(), skill(false, false, false)),
            ("Frost Bomb".to_owned(), skill(false, false, true)),
        ]
        .into();
        let item_only = "You do not have enough Mana to use: The Stars Answer, The Stars Answer";
        assert!(warning(item_only.into(), &skills).is_none());

        let main = warning(
            "You do not have enough Life to use: Spark, The Stars Answer".into(),
            &skills,
        )
        .unwrap();
        assert_eq!(main.severity, Severity::Problem);
        assert_eq!(main.title, "Not enough Life to use Spark");

        let utility = warning(
            "You do not have enough Mana to use: Frost Bomb".into(),
            &skills,
        )
        .unwrap();
        assert_eq!(utility.severity, Severity::Note);
    }

    #[test]
    fn new_warnings_name_only_newly_affected_used_skills() {
        let skills: HashMap<String, SkillUse> = [
            (
                "Spark".to_owned(),
                SkillUse {
                    main: true,
                    full_dps: true,
                    socketed: true,
                },
            ),
            ("Frost Bomb".to_owned(), SkillUse::default()),
        ]
        .into();
        let before = vec!["You do not have enough Mana to use: Frost Bomb".to_owned()];
        let after = vec![
            "You do not have enough Mana to use: Spark, Frost Bomb".to_owned(),
            "You do not meet the Strength requirement of Spark".to_owned(),
        ];
        assert_eq!(
            new_warnings(&before, &after, &skills),
            [
                "Not enough Mana to use Spark",
                "You do not meet the Strength requirement of Spark"
            ]
        );
        assert!(new_warnings(&before, &before, &skills).is_empty());
    }

    #[test]
    fn chaos_resistance_is_raised_by_one_mod() {
        let roll = (4.0, 27.0, "Ring".to_owned());
        assert_eq!(resist_step("Chaos", 37.0, Some(&roll)), 16.0);
        assert_eq!(resist_step("Chaos", 5.0, Some(&roll)), 5.0);
        assert_eq!(resist_step("Lightning", 49.0, Some(&roll)), 49.0);
    }

    #[test]
    fn keeps_only_finite_changes() {
        let sheet = |v: &str| {
            StatSheet::from_build_xml(&format!(
                r#"<PathOfBuilding2><Build><PlayerStat stat="TotalEHP" value="{v}"/></Build></PathOfBuilding2>"#
            ))
            .unwrap()
        };
        let stats = [("EHP".to_owned(), "TotalEHP".to_owned())];
        assert!(measured(&stats, &sheet("100"), &sheet("100")).is_empty());
        assert!(measured(&stats, &sheet("inf"), &sheet("inf")).is_empty());
        assert_eq!(
            measured(&stats, &sheet("100"), &sheet("150"))[0].change,
            50.0
        );
    }

    #[test]
    fn accuracy_search_finds_the_threshold_within_one_percent() {
        for threshold in [1, 99, 100, 101, 777, 15_075, 25_600] {
            let mut search = AccuracySearch::default();
            let mut rounds = 0;
            let mut points: Vec<u32> =
                std::iter::successors(Some(100), |n| (*n < ACCURACY_LIMIT).then(|| n * 2))
                    .collect();
            while !points.is_empty() {
                rounds += 1;
                for p in points {
                    search.record(p, p >= threshold);
                }
                points = search.next(8);
            }
            let found = search.high.unwrap();
            assert!(found >= threshold, "{threshold}: {found}");
            assert!(
                found - threshold <= (found / 100).max(1),
                "{threshold}: {found}"
            );
            assert!(rounds <= 4, "{threshold}: {rounds} rounds");
        }
        let mut never = AccuracySearch::default();
        never.record(ACCURACY_LIMIT, false);
        assert!(never.next(8).is_empty() && never.high.is_none());
    }

    #[test]
    fn writes_mod_lines_pob_reads() {
        assert_eq!(
            resist_line("Lightning", 49.0),
            "+49% to Lightning Resistance"
        );
        assert_eq!(group(25_600), "25,600");
        assert_eq!(fmt(26.0), "26");
    }
}
