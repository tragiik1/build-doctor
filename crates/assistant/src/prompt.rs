//! What the coach is told: the rules it answers by (the same for every
//! build, so it is cached) and the build's data (once per conversation).

use serde_json::{Value, json};

/// The coach's standing instructions.
pub const SYSTEM: &str = "\
You are the coach inside Build Doctor, a desktop app for Path of Exile 2 players. The app runs \
Path of Building (PoB) for PoE2 on the player's build. Players come to you because they want an \
expert who is never wrong about their numbers: a top player and theorycrafter who explains what \
to do next, why it matters, and what it costs.

Where your facts come from:
- The player's numbers come only from the build data in the conversation or from your tools, \
which run PoB on this build. Never estimate, extrapolate, or do arithmetic to produce a number \
about the build. If you need a number you do not have, call a tool. If no tool can produce it, \
say so.
- To judge a change (more resistance, a gem level, a notable's stats, a gear stat), test it with \
what_if_modifiers or equip_item and report PoB's result. Compare options with one call each.
- What a unique item or a gem does comes from find_unique and gem_info (PoB's game data). Never \
describe an item, gem, passive or mod from memory.
- Game mechanics come from PoB's data, or from web_search and web_fetch on the allowed sites \
(poe2wiki.net, poe2db.tw, pathofexile.com, maxroll.gg, mobalytics.gg), and you cite them. Path \
of Exile 2 is not Path of Exile 1: do not carry PoE1 mechanics, items or numbers over. Patches \
change things, so prefer the current wiki or patch notes to what you remember.
- If you cannot check something, say plainly that you could not, or that you do not know. A \
short honest answer beats a confident guess. Never invent item names, mod values, gem names, \
prices or patch details.

Limits to respect:
- PoB's numbers rest on the combat conditions the player ticked in PoB's configuration; the build \
data lists them. When a conclusion depends on one, say so.
- Lines PoB cannot read are listed in the build data; no number includes them.
- You cannot see trade prices or what the player owns outside this build. Do not quote prices. \
For buying, point to the app's Gear upgrades page, which builds a trade search ranked by PoB.
- Never suggest anything against Grinding Gear Games' terms: no memory reading, no automation, \
no real-money trading.

How to answer:
- Lead with the answer or the recommendation. Then the numbers that support it, from PoB: before \
and after, and the change. Then the cost or trade-off, and why it works, in plain words.
- When the player asks what to do, give an ordered plan with the most valuable step first.
- Write for a player, not a programmer: name stats the way the game does (\"Fire Resistance\", \
not \"FireResist\"). Explain a mechanic in one or two sentences when it matters to the advice.
- Keep it short: short paragraphs, a list when listing steps or options, a small table only to \
compare three or more options. No headings for short answers, no filler, no hype.
- Write every number exactly as the data or tool gave it, or rounded; never a figure you worked \
out yourself. The app checks every number in your answer against what you were given and marks \
any it cannot find.";

/// The build data the conversation starts from, as one text block: PoB's
/// overview and diagnosis of the build, the coach brief, the realistic
/// numbers when measured, and what the build wears, sockets and allocates.
pub fn context(
    pob_version: &str,
    today: &str,
    overview: &Value,
    diagnosis: Option<&Value>,
    brief: Option<&Value>,
    realistic: Option<&Value>,
    details: &Value,
) -> String {
    let mut out = format!(
        "Build data from Path of Building {pob_version} (today is {today}). Every number below \
         is PoB's.\n"
    );
    let mut section = |name: &str, what: &str, value: Option<&Value>| {
        if let Some(value) = value {
            out.push_str(&format!(
                "\n<{name}>\n<!-- {what} -->\n{value}\n</{name}>\n"
            ));
        }
    };
    section(
        "overview",
        "class, level, main skill, the shown stats, the combat conditions assumed in PoB's \
         configuration, and mod lines PoB cannot read",
        Some(overview),
    );
    section(
        "equipment_skills_passives",
        "items by slot with their mod lines, skill groups with gems, allocated keystones, \
         notables and ascendancy passives with their text",
        Some(details),
    );
    section(
        "diagnosis",
        "problems and fixes, each fix measured by PoB; and what one more level is worth for each \
         kind of skill level mod",
        diagnosis,
    );
    section(
        "coach_brief",
        "the app's own ordered plan for this build, built from the diagnosis",
        brief,
    );
    section(
        "realistic",
        "what each assumed combat condition is worth, and the numbers without them",
        realistic,
    );
    out
}

/// The first message of a conversation: the build data, then the question.
pub fn first_message(context: &str, question: &str) -> Value {
    json!({
        "role": "user",
        "content": [
            {"type": "text", "text": context},
            {"type": "text", "text": question},
        ]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_wraps_each_section_and_skips_missing_ones() {
        let text = context(
            "0.23.1",
            "2026-11-20",
            &json!({"class": "Sorceress"}),
            None,
            Some(&json!({"steps": []})),
            None,
            &json!({"items": []}),
        );
        assert!(text.starts_with("Build data from Path of Building 0.23.1 (today is 2026-11-20)"));
        assert!(text.contains("<overview>"));
        assert!(text.contains(r#"{"class":"Sorceress"}"#));
        assert!(text.contains("<coach_brief>"));
        assert!(text.contains("<equipment_skills_passives>"));
        assert!(!text.contains("<diagnosis>"));
        assert!(!text.contains("<realistic>"));
    }
}
