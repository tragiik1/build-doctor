//! `doctor ask`: a question to the AI coach about a build, with the
//! player's own Anthropic API key (`ANTHROPIC_API_KEY`), billed to that
//! account.

use std::io::Write;
use std::process::ExitCode;
use std::sync::atomic::AtomicBool;

use assistant::{Bench, Client, Conversation, Progress};

use crate::AnyResult;

pub fn run(pool: &pob::Pool, xml: &str, question: &str, model: &str) -> AnyResult<ExitCode> {
    let key = std::env::var("ANTHROPIC_API_KEY").map_err(|_| {
        "Set ANTHROPIC_API_KEY to your Anthropic API key (console.anthropic.com). Questions are billed to that account."
    })?;
    // For tests against a local server.
    let base = std::env::var("BUILD_DOCTOR_API_URL").ok();
    let client = Client::new(&key, base.as_deref());

    let overview = analysis::overview(pool, xml)?;
    let diagnosis = analysis::diagnose(pool, xml)?;
    let brief = analysis::brief(&overview, &diagnosis, None);
    let bench = Bench { pool, xml };
    let context = assistant::build_context(bench, &overview, Some(&diagnosis), Some(&brief), None)?;
    let mut conversation = Conversation::new(context);

    let mut out = std::io::stdout();
    let answer = conversation.ask(
        &client,
        model,
        bench,
        question,
        &AtomicBool::new(false),
        &mut |p| match p {
            Progress::Text(text) => {
                let _ = write!(out, "{text}");
                let _ = out.flush();
            }
            Progress::Step(step) => eprintln!("· {step}"),
        },
    )?;
    println!();
    if !answer.sources.is_empty() {
        println!();
        println!("Sources");
        for (i, s) in answer.sources.iter().enumerate() {
            println!("  [{}] {} {}", i + 1, s.title, s.url);
        }
    }
    if !answer.unverified.is_empty() {
        println!();
        println!(
            "Not found in PoB's results or the cited sources: {}",
            answer.unverified.join(", ")
        );
    }
    if let Some(note) = &answer.note {
        println!();
        println!("{note}");
    }
    println!();
    let tokens = answer.usage.input + answer.usage.cache_write + answer.usage.cache_read;
    match answer.cost {
        Some(cost) => println!(
            "{}: {} tokens in, {} out, {} searches, ${cost:.3}",
            answer.model, tokens, answer.usage.output, answer.usage.web_searches
        ),
        None => println!(
            "{}: {} tokens in, {} out, {} searches",
            answer.model, tokens, answer.usage.output, answer.usage.web_searches
        ),
    }
    Ok(ExitCode::SUCCESS)
}
