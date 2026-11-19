//! `doctor item`: prints [`analysis::item_check`].

use std::process::ExitCode;

use analysis::diagnose::Effect;
use pob::Pool;

use crate::AnyResult;
use crate::check::{format_number, signed};

pub fn run(pool: &Pool, xml: &str, item: &str) -> AnyResult<ExitCode> {
    let r = analysis::item_check(pool, xml, item)?;
    println!("{}", r.build.describe());
    println!("PoB2 {}", r.build.pob_version);
    println!();
    println!("{} ({})", r.item, r.rarity.to_lowercase());
    for line in &r.unread {
        println!("  PoB cannot read: {line}");
    }
    if r.slots.is_empty() {
        println!("PoB found no equipment slot for this item.");
        return Ok(ExitCode::SUCCESS);
    }
    for s in &r.slots {
        println!();
        println!(
            "{} instead of {}: {}",
            s.slot,
            s.replaces.as_deref().unwrap_or("nothing"),
            s.verdict
        );
        for e in s.headline.iter().chain(&s.changes) {
            println!("  {}", effect(e));
        }
        for w in &s.new_warnings {
            println!("  PoB warns: {w}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn effect(e: &Effect) -> String {
    if e.percent {
        format!("{}: {}% -> {}%", e.label, e.before, e.after)
    } else {
        format!(
            "{}: {} -> {} ({})",
            e.label,
            format_number(e.before),
            format_number(e.after),
            signed(e.change)
        )
    }
}
