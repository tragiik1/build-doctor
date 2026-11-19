//! Build analyses on top of PoB. Every number comes from PoB; this crate
//! decides what to calculate and returns plain data that front ends (the CLI,
//! the desktop app) only present.
//!
//! - [`diagnose`]: what is wrong with a build, each fix measured by PoB.
//! - [`overview`]: a build as PoB sees it: stats, assumptions, ignored mods,
//!   and whether PoB reproduces the build's saved numbers.
//! - [`realistic`]: what each assumed combat condition is worth, and the
//!   numbers without them.
//! - [`plan`]: a step-by-step passive tree plan, each step exact.

pub mod diagnose;
pub mod overview;
pub mod plan;
pub mod realistic;

use pob::Summary;
use serde::Serialize;

pub use diagnose::{Diagnosis, Finding, Severity, diagnose};
pub use overview::{Overview, overview};
pub use plan::{Goal, Plan, PlanOptions, PlanProgress, plan};
pub use realistic::{Realistic, realistic};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Pob(#[from] pob::Error),
    #[error("{0}")]
    Analysis(String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Percent change from `before` to `after`; 0 when `before` is 0.
pub fn pct(before: f64, after: f64) -> f64 {
    if before == 0.0 {
        0.0
    } else {
        (after - before) / before * 100.0
    }
}

/// Damage as the build measures it: Full DPS when it uses Full DPS,
/// otherwise the main skill's and minions' combined DPS.
pub fn damage(uses_full_dps: bool, s: &Summary) -> f64 {
    if uses_full_dps {
        s.full_dps
    } else {
        s.combined_dps + s.minion_dps
    }
}

/// A configuration option a build's numbers depend on.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssumptionRow {
    pub var: String,
    /// "condition" (a state kept up in a fight) or "scenario" (the fight
    /// simulated and how PoB calculates it).
    pub kind: &'static str,
    pub section: String,
    pub label: String,
    pub value: String,
}

impl From<&pob::Assumption> for AssumptionRow {
    fn from(a: &pob::Assumption) -> Self {
        Self {
            var: a.var.clone(),
            kind: match a.kind {
                pob::AssumptionKind::Condition => "condition",
                pob::AssumptionKind::Scenario => "scenario",
            },
            section: a.section.clone(),
            label: a.label.clone(),
            value: a.value.clone(),
        }
    }
}

/// Who a build is.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub class: String,
    pub ascendancy: Option<String>,
    pub level: Option<u32>,
    pub main_skill: Option<String>,
    pub tree_version: Option<String>,
    pub latest_tree_version: Option<String>,
    pub tree_outdated: bool,
    pub pob_version: String,
}

impl BuildInfo {
    pub fn new(id: &pob::Identity, pob_version: &str) -> Self {
        Self {
            class: id.class.clone().unwrap_or_else(|| "Unknown class".into()),
            ascendancy: id.ascendancy.clone().filter(|a| a != "None"),
            level: id.level,
            main_skill: id.main_skill.clone(),
            tree_version: id.tree_version.clone(),
            latest_tree_version: id.latest_tree_version.clone(),
            tree_outdated: id.tree_is_outdated(),
            pob_version: pob_version.to_owned(),
        }
    }

    /// "Shaman (Druid), level 94, main skill Snap".
    pub fn describe(&self) -> String {
        let who = match &self.ascendancy {
            Some(asc) => format!("{asc} ({})", self.class),
            None => self.class.clone(),
        };
        format!(
            "{who}, level {}, main skill {}",
            self.level.map_or("-".into(), |l| l.to_string()),
            self.main_skill.as_deref().unwrap_or("-"),
        )
    }
}

/// A build as PoB sees it, read through one pool worker.
pub(crate) struct Inspected {
    pub id: pob::Identity,
    pub assumptions: Vec<pob::Assumption>,
    pub xml: String,
    pub stats: pob::StatSheet,
}

pub(crate) fn inspect(pool: &pob::Pool, xml: &str) -> Result<Inspected> {
    let mut found = pool.map(xml, None, vec![()], |engine, ()| {
        Ok((
            engine.identity()?,
            engine.assumptions()?,
            engine.ensure_original()?,
        ))
    })?;
    let (id, assumptions, saved) = found.pop().expect("one item in, one out");
    let stats = pob::StatSheet::from_build_xml(&saved)?;
    Ok(Inspected {
        id,
        assumptions,
        xml: saved,
        stats,
    })
}

/// The build's combat conditions: config options a player has to keep up.
pub(crate) fn conditions(assumptions: &[pob::Assumption]) -> Vec<&pob::Assumption> {
    assumptions
        .iter()
        .filter(|a| a.kind == pob::AssumptionKind::Condition)
        .collect()
}

/// Checks every kept condition variable exists in the build.
pub(crate) fn check_kept(conditions: &[&pob::Assumption], kept: &[String]) -> Result<()> {
    for var in kept {
        if !conditions.iter().any(|c| &c.var == var) {
            return Err(Error::Analysis(format!(
                "{var} is not a condition of this build"
            )));
        }
    }
    Ok(())
}
