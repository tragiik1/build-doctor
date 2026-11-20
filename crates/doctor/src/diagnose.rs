//! `doctor diagnose`: prints [`analysis::diagnose`].

use std::process::ExitCode;

use analysis::Severity;
use pob::Pool;

use crate::AnyResult;
use crate::check::{format_number, signed};

pub fn run(pool: &Pool, xml: &str) -> AnyResult<ExitCode> {
    let d = analysis::diagnose(pool, xml)?;
    println!("{}", d.build.describe());
    println!("PoB2 {}", d.build.pob_version);
    println!();
    if d.findings.is_empty() {
        println!("Nothing to fix: PoB finds no problems and no measurable gains.");
    }
    for f in &d.findings {
        let tag = match f.severity {
            Severity::Problem => "Problem",
            Severity::Gain => "Fix",
            Severity::Note => "Note",
        };
        println!("{tag:<8} {}", f.title);
        if let Some(fix) = &f.fix {
            println!("{:<8} Needs {fix}", "");
        }
        for e in &f.effects {
            if e.percent {
                println!("{:<8} {}: {}% -> {}%", "", e.label, e.before, e.after);
            } else {
                println!(
                    "{:<8} {}: {} -> {} ({})",
                    "",
                    e.label,
                    format_number(e.before),
                    format_number(e.after),
                    signed(e.change)
                );
            }
        }
        if let Some(detail) = &f.detail {
            println!("{:<8} {detail}", "");
        }
        println!();
    }
    if !d.levels.is_empty() {
        println!("One more skill level:");
        for l in &d.levels {
            println!(
                "  {:<44} {} {}",
                l.line,
                l.damage.label,
                signed(l.damage.change)
            );
            for w in &l.new_warnings {
                println!("  {:<44} PoB warns: {w}", "");
            }
        }
    }
    Ok(ExitCode::SUCCESS)
}
