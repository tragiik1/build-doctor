//! `doctor gear`: prints [`analysis::gear`] and [`analysis::slot_values`].

use std::process::ExitCode;

use analysis::TradeOptions;
use analysis::gear::{Goal, Share};
use pob::Pool;

use crate::{AnyResult, print_json};

pub fn run(
    pool: &Pool,
    xml: &str,
    slot: Option<&str>,
    trade: Option<&TradeOptions>,
    json: bool,
) -> AnyResult<ExitCode> {
    let Some(slot) = slot else {
        let g = analysis::gear(pool, xml)?;
        if json {
            return print_json(&g);
        }
        println!("{}", g.build.describe());
        println!();
        for s in &g.slots {
            println!("{:<14} {}", s.slot, s.item);
        }
        println!();
        println!("Measure one with --slot \"<slot>\".");
        return Ok(ExitCode::SUCCESS);
    };
    let v = analysis::slot_values(pool, xml, slot)?;
    let url = trade.map(|t| analysis::trade_url(&v, t)).transpose()?;
    if json {
        return print_json(&serde_json::json!({ "values": v, "tradeUrl": url }));
    }
    println!("{}: {}", v.slot.slot, v.slot.item);
    println!(
        "It adds, over a blank rare of the same base: {}",
        shares(&v.worn_item)
    );
    println!();
    println!(
        "Worth on a new {} ({} of {} stats add something, each alone at a mid roll):",
        v.slot.category,
        v.stats.len(),
        v.measured
    );
    for s in v.stats.iter().take(25) {
        let line = if s.implicit {
            format!("{} (implicit)", s.line)
        } else {
            s.line.clone()
        };
        println!("  {line:<52} {}", shares(&s.share));
    }
    if let Some(url) = url {
        println!();
        println!(
            "Trade search ({}):",
            goal_name(trade.map_or(Goal::Balanced, |t| t.goal))
        );
        println!("{url}");
    }
    Ok(ExitCode::SUCCESS)
}

fn shares(s: &Share) -> String {
    format!(
        "damage {:+.2}%, EHP {:+.2}%, min hit {:+.2}%",
        s.damage, s.ehp, s.min_hit
    )
}

fn goal_name(goal: Goal) -> &'static str {
    match goal {
        Goal::Damage => "damage",
        Goal::Survival => "survival",
        Goal::Balanced => "damage and survival",
    }
}
