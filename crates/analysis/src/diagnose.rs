//! What is wrong with a build, most important first.
//!
//! Every finding is a number PoB calculates, and every gain is PoB
//! recalculating the build with the fix applied as a line in its Custom
//! Modifiers box (for example "+49% to Lightning Resistance"), so a fix is
//! worth exactly what PoB says it is. A fix PoB does not confirm (the
//! resistance does not reach its maximum, hit chance does not reach 100%) is
//! reported without a gain, and a fix that changes none of PoB's numbers is
//! not reported.

use pob::{Outputs, Pool, StatKind, StatSheet};
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
}

/// One PoB recalculation (or search) to run in the pool.
enum Job {
    /// Raise a resistance to its maximum.
    Resist {
        element: &'static str,
        now: f64,
        missing: f64,
    },
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
        reached: bool,
        stats: StatSheet,
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
            });
        }
    }
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
        } => {
            let key = format!("{element}Resist");
            let (calc, after) = engine.with_mods(&[&resist_line(element, missing)], &[&key])?;
            Ok(Outcome::Resist {
                element,
                now,
                missing,
                reached: after.get(&key).is_some_and(|v| *v >= now + missing),
                stats: calc.stats,
            })
        }
        Job::Penalty => Ok(Outcome::Penalty(
            engine.without_config(&["resistancePenalty"])?.stats,
        )),
        Job::Accuracy(added) => probe_accuracy(engine, added),
    })?;

    let mut findings: Vec<Finding> = Vec::new();
    let mut search = AccuracySearch::default();
    for outcome in outcomes {
        findings.extend(match outcome {
            Outcome::Penalty(stats) => penalty(&read.assumptions, &base, &stats),
            Outcome::Resist {
                element,
                now,
                missing,
                reached,
                stats,
            } => resist(element, now, missing, reached, &base, &stats),
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
    findings.extend(read.warnings.into_iter().map(warning));
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

    Ok(Diagnosis {
        build: BuildInfo::new(&read.id, &pool.install().version().to_string()),
        findings,
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

/// A resistance below its maximum. Nothing when raising it changes none of
/// PoB's numbers (an immunity, for example).
fn resist(
    element: &str,
    now: f64,
    missing: f64,
    reached: bool,
    base: &StatSheet,
    fixed: &StatSheet,
) -> Option<Finding> {
    let title = format!(
        "{element} resistance {}%, maximum {}%",
        fmt(now),
        fmt(now + missing)
    );
    if !reached {
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
    (!effects.is_empty()).then(|| Finding {
        severity: Severity::Gain,
        title,
        detail: None,
        fix: Some(resist_line(element, missing)),
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

/// A PoB warning. Spare weapon-set points are worth knowing; everything
/// else PoB warns about is something the game would not allow, so the
/// numbers include something the character cannot have.
fn warning(text: String) -> Finding {
    if text.contains("passives available") {
        return note(text, "Unspent points on the second weapon set.".into());
    }
    Finding {
        severity: Severity::Problem,
        title: text,
        detail: Some(
            "PoB's own warning. The game would not allow this, so the numbers include something the character cannot have."
                .into(),
        ),
        fix: None,
        effects: Vec::new(),
    }
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
        let spare = warning("You have 7 Weapon set 2 passives available".into());
        assert_eq!(spare.severity, Severity::Note);
        let spirit = warning("Your unreserved Spirit is negative".into());
        assert_eq!(spirit.severity, Severity::Problem);
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
