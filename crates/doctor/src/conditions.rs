//! What `realistic` and `todo` share: reading a build's identity and
//! assumptions through the pool, and the player's `--keep` choices.

use pob::{Assumption, AssumptionKind, Calculation, Identity, Pool};

use crate::AnyResult;

/// A build as PoB sees it.
pub struct Inspected {
    pub id: Identity,
    pub assumptions: Vec<Assumption>,
    pub calc: Calculation,
}

pub fn inspect(pool: &Pool, xml: &str) -> AnyResult<Inspected> {
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
        calc: Calculation {
            xml: saved,
            stats,
            passes: 0,
        },
    })
}

/// The conditions a player says they really sustain, matched by
/// case-insensitive text in the condition's label.
pub struct Keep {
    patterns: Vec<String>,
}

impl Keep {
    /// Fails if a pattern matches none of `conditions`, listing them, so a
    /// typo never silently drops a condition.
    pub fn new(patterns: &[String], conditions: &[&Assumption]) -> AnyResult<Self> {
        let keep = Self {
            patterns: patterns.iter().map(|p| p.to_lowercase()).collect(),
        };
        for (pattern, original) in keep.patterns.iter().zip(patterns) {
            if !conditions
                .iter()
                .any(|c| c.label.to_lowercase().contains(pattern))
            {
                let labels: Vec<&str> = conditions.iter().map(|c| c.label.as_str()).collect();
                return Err(format!(
                    "--keep {original:?} matches no condition of this build. Conditions: {}",
                    labels.join("; ")
                )
                .into());
            }
        }
        Ok(keep)
    }

    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    pub fn keeps(&self, label: &str) -> bool {
        let label = label.to_lowercase();
        self.patterns.iter().any(|p| label.contains(p))
    }
}

/// The build's conditions (states to keep up in a fight).
pub fn conditions(assumptions: &[Assumption]) -> Vec<&Assumption> {
    assumptions
        .iter()
        .filter(|a| a.kind == AssumptionKind::Condition)
        .collect()
}
