//! The player's `--keep` choices: text matched against condition labels.

use analysis::AssumptionRow;
use pob::Pool;

use crate::AnyResult;

/// Resolves `--keep` texts (case-insensitive, matched within condition
/// labels) to PoB config variables. A text that matches no condition is an
/// error listing the conditions, so a typo never silently drops one.
pub fn kept_vars(pool: &Pool, xml: &str, patterns: &[String]) -> AnyResult<Vec<String>> {
    if patterns.is_empty() {
        return Ok(Vec::new());
    }
    let overview = analysis::overview(pool, xml)?;
    let conditions: Vec<&AssumptionRow> = overview
        .assumptions
        .iter()
        .filter(|a| a.kind == "condition")
        .collect();
    let mut vars = Vec::new();
    for pattern in patterns {
        let needle = pattern.to_lowercase();
        let matched: Vec<&&AssumptionRow> = conditions
            .iter()
            .filter(|c| c.label.to_lowercase().contains(&needle))
            .collect();
        if matched.is_empty() {
            let labels: Vec<&str> = conditions.iter().map(|c| c.label.as_str()).collect();
            return Err(format!(
                "--keep {pattern:?} matches no condition of this build. Conditions: {}",
                labels.join("; ")
            )
            .into());
        }
        vars.extend(matched.into_iter().map(|c| c.var.clone()));
    }
    vars.sort();
    vars.dedup();
    Ok(vars)
}
