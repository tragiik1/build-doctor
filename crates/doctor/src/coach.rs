//! `doctor coach`: prints [`analysis::brief`].

use std::process::ExitCode;

use analysis::Brief;

use crate::AnyResult;

pub fn run(brief: &Brief) -> AnyResult<ExitCode> {
    for line in &brief.verdict {
        wrap("", line);
    }
    println!();
    println!("What to do, in order");
    for (i, s) in brief.steps.iter().enumerate() {
        println!();
        println!("{}. {}", i + 1, s.title);
        wrap("   Why  ", &s.why);
        wrap("   How  ", &s.how);
        if let Some(gain) = &s.gain {
            wrap("   PoB  ", gain);
        }
        if let Some(src) = &s.source {
            wrap("   More ", &format!("{} {}", src.label, src.url));
        }
    }
    for (title, points) in [("Damage", &brief.damage), ("Defence", &brief.defence)] {
        println!();
        println!("{title}");
        for p in points {
            wrap("   ", &format!("{}: {}", p.title, p.text));
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Prints `text` wrapped at 96 columns, the first line after `label` and the
/// rest indented to match.
fn wrap(label: &str, text: &str) {
    const WIDTH: usize = 96;
    let indent = " ".repeat(label.chars().count());
    let mut line = label.to_owned();
    for word in text.split(' ') {
        if line.chars().count() + word.chars().count() > WIDTH
            && line.trim().len() > label.trim().len()
        {
            println!("{}", line.trim_end());
            line = indent.clone();
        }
        line.push_str(word);
        line.push(' ');
    }
    println!("{}", line.trim_end());
}
