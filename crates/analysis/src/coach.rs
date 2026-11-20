//! The coach: what the numbers mean and what to do, in order, in plain
//! words.
//!
//! Built only from what the other analyses computed (the overview, the
//! diagnosis and, when available, the realistic numbers); it makes no PoB
//! calls. Every number in its text is one of those results. Explanations of
//! game mechanics are general and match public sources (linked on each
//! step); nothing here is a guess about the game or the build.

use serde::Serialize;

use crate::diagnose::{Diagnosis, Finding, LevelWorth, Severity};
use crate::overview::Overview;
use crate::realistic::Realistic;
use pob::RollRange;

/// Steps the plan shows at most.
const MAX_STEPS: usize = 6;
/// A skill level worth less than this (percent damage) is not a step.
const LEVEL_STEP: f64 = 3.0;
/// Armour or evasion below this is too little to be worth a line.
const MITIGATION_SHOWN: f64 = 100.0;
/// The smallest max hit makes a step when it is below this share of the
/// next smallest.
const WEAK_POINT: f64 = 0.6;
/// Conditions taking more than this (percent damage) off make a step.
const CONDITIONS_STEP: f64 = 20.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Brief {
    /// The build in a few plain sentences.
    pub verdict: Vec<String>,
    /// What to do, most important first.
    pub steps: Vec<Step>,
    /// How the build deals damage.
    pub damage: Vec<Point>,
    /// How the build stays alive.
    pub defence: Vec<Point>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Step {
    /// What to do, as an instruction.
    pub title: String,
    /// Why it matters: the mechanic and this build's numbers.
    pub why: String,
    /// How to do it.
    pub how: String,
    /// What PoB says it changes, when measured.
    pub gain: Option<String>,
    /// The app page that does it: "item", "gear", "plan", "realistic".
    pub tool: Option<&'static str>,
    /// Where the mechanic is explained.
    pub source: Option<Source>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Source {
    pub label: &'static str,
    pub url: &'static str,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Point {
    pub title: String,
    pub text: String,
}

const RESISTANCE: Source = Source {
    label: "Resistance (PoE2 wiki)",
    url: "https://www.poe2wiki.net/wiki/Resistance",
};
const CHAOS: Source = Source {
    label: "Chaos resistance (PoE2 wiki)",
    url: "https://www.poe2wiki.net/wiki/Chaos_resistance",
};
const ACCURACY: Source = Source {
    label: "Accuracy (PoE2 wiki)",
    url: "https://www.poe2wiki.net/wiki/Accuracy",
};
const ARMOUR: Source = Source {
    label: "Armour (Mobalytics)",
    url: "https://mobalytics.gg/poe-2/guides/armour",
};
const SPIRIT: Source = Source {
    label: "Spirit guide (Maxroll)",
    url: "https://maxroll.gg/poe2/resources/spirit-guide",
};

/// The brief for a build. `realistic` is optional: without it the plan has
/// no reality-check step and the damage section does not list conditions.
pub fn brief(overview: &Overview, diagnosis: &Diagnosis, realistic: Option<&Realistic>) -> Brief {
    let stats = Stats(overview);
    let mut steps: Vec<Step> = Vec::new();

    // 1. Anything that makes PoB's numbers wrong for the character.
    for f in diagnosis
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Problem)
    {
        steps.push(problem_step(f));
    }
    // 2. Measured defensive fixes, largest first (the diagnosis ranks them).
    let gains: Vec<&Finding> = diagnosis
        .findings
        .iter()
        .filter(|f| f.severity == Severity::Gain)
        .collect();
    for f in &gains {
        if let Some(step) = resistance_step(f, &stats) {
            steps.push(step);
        }
    }
    // The weakest hit type, when it lags far behind the rest and no
    // resistance step above already covers it.
    if let Some(step) = weak_point_step(&stats, &steps) {
        steps.push(step);
    }
    // 3. Damage: accuracy, then skill levels.
    for f in &gains {
        if f.title.starts_with("Hit chance") {
            steps.push(accuracy_step(f));
        }
    }
    if let Some(level) = best_level(&diagnosis.levels) {
        steps.push(level_step(level));
    }
    // 4. Whether the damage is real.
    if let Some(step) = realistic.and_then(conditions_step) {
        steps.push(step);
    }
    // 5. The two tools that always have something to offer.
    steps.push(Step {
        title: "Find free gains in your passive tree".into(),
        why: "Respeccing passives costs only gold. The passive tree plan tries swapping your weakest points for better ones within reach, keeping the same number of points, and only keeps a swap that Path of Building confirms.".into(),
        how: "Open Passive tree plan, pick damage or survival, and build the plan. Each step lists what to take and what to drop.".into(),
        gain: None,
        tool: Some("plan"),
        source: None,
    });
    steps.push(Step {
        title: "Find your next gear upgrade".into(),
        why: "Path of Building measures what every stat that can roll on a slot is worth to your build, so you know what to look for instead of guessing.".into(),
        how: "Open Gear upgrades, pick a slot, and open the trade search: it lists items for sale best first. Paste a promising one into Check an item for its exact value.".into(),
        gain: None,
        tool: Some("gear"),
        source: None,
    });
    steps.truncate(MAX_STEPS);

    Brief {
        verdict: verdict(overview, &stats, realistic, steps.first()),
        damage: damage_points(overview, &stats, diagnosis, realistic),
        defence: defence_points(&stats),
        steps,
    }
}

/// Reads the overview's stats by PoB name.
struct Stats<'a>(&'a Overview);

impl Stats<'_> {
    fn get(&self, stat: &str) -> Option<f64> {
        self.0
            .groups
            .iter()
            .flat_map(|g| &g.rows)
            .find(|r| r.stat == stat)
            .map(|r| r.value)
    }

    /// The build's damage number and its name.
    fn damage(&self) -> (f64, &'static str) {
        match self.get("FullDPS").filter(|v| *v > 0.0) {
            Some(v) => (v, "Full DPS"),
            None => (
                self.get("CombinedDPS").unwrap_or(0.0) + self.0.minion_dps.unwrap_or(0.0),
                "DPS",
            ),
        }
    }

    /// Max hit per damage type, smallest first, finite values only.
    fn max_hits(&self) -> Vec<(&'static str, f64)> {
        let mut hits: Vec<(&'static str, f64)> = ["Physical", "Fire", "Cold", "Lightning", "Chaos"]
            .iter()
            .filter_map(|t| {
                self.get(&format!("{t}MaximumHitTaken"))
                    .filter(|v| v.is_finite())
                    .map(|v| (*t, v))
            })
            .collect();
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));
        hits
    }
}

fn verdict(
    overview: &Overview,
    stats: &Stats<'_>,
    realistic: Option<&Realistic>,
    first: Option<&Step>,
) -> Vec<String> {
    let b = &overview.build;
    let who = match &b.ascendancy {
        Some(a) => format!("{a} ({})", b.class),
        None => b.class.clone(),
    };
    let (dmg, label) = stats.damage();
    let mut out = vec![format!(
        "{who}{} using {}. Path of Building puts your damage at {} {label}.",
        b.level.map(|l| format!(", level {l},")).unwrap_or_default(),
        b.main_skill.as_deref().unwrap_or("your main skill"),
        number(dmg)
    )];
    if let Some(r) = realistic
        && let Some(i) = r.metrics.iter().position(|m| m.damage)
        && r.configured[i] > 0.0
    {
        let share = 100.0 - r.floor[i] / r.configured[i] * 100.0;
        if share >= 5.0 {
            out.push(format!(
                "{} of that depends on combat conditions Path of Building assumes are always true, such as an enemy being shocked; without any of them it is {}.",
                percent(share),
                number(r.floor[i])
            ));
        }
    }
    let hits = stats.max_hits();
    if let (Some(weak), Some(strong)) = (hits.first(), hits.last()) {
        let ehp = stats
            .get("TotalEHP")
            .map(|e| format!(" Your effective hit pool is {}.", number(e)))
            .unwrap_or_default();
        out.push(format!(
            "The biggest single hit you survive is {} {} damage; {} is your weakest point, against {} for {}.{ehp}",
            number(weak.1),
            weak.0.to_lowercase(),
            weak.0.to_lowercase(),
            number(strong.1),
            strong.0.to_lowercase(),
        ));
    }
    if let Some(step) = first {
        out.push(format!("First priority: {}.", lowercase_first(&step.title)));
    }
    out
}

fn problem_step(f: &Finding) -> Step {
    let t = &f.title;
    if t.starts_with("PoB uses the") {
        return Step {
            title: "Set the endgame resistance penalty in Path of Building".into(),
            why: format!(
                "The game lowers all your elemental resistances by 60% from the endgame on. {t}, so every defence number in this build is higher than you would really have there."
            ),
            how: "In Path of Building, open Configuration, set the elemental resistance penalty to Endgame (-60%), and load the build here again.".into(),
            gain: effects(f),
            tool: None,
            source: Some(RESISTANCE),
        };
    }
    if t.contains("too many gem groups") {
        let numbers: Vec<&str> = t
            .split(|c: char| !c.is_ascii_digit())
            .filter(|s| !s.is_empty())
            .collect();
        let (allowed, have) = (numbers.first().copied(), numbers.get(1).copied());
        return Step {
            title: match allowed {
                Some(n) => format!("Cut your skill groups down to {n}"),
                None => "Cut your skill groups down to the limit".into(),
            },
            why: format!(
                "The game only lets you use {} skill groups, but this build has {}. Path of Building still counts every group, so your numbers include skills or supports you could not actually have.",
                allowed.unwrap_or("a limited number of"),
                have.unwrap_or("more")
            ),
            how: "In Path of Building, disable or remove the groups you don't use in game, then load the build here again.".into(),
            gain: None,
            tool: None,
            source: None,
        };
    }
    if t.contains("Spirit is negative") {
        return Step {
            title: "Bring your Spirit back above zero".into(),
            why: "Spirit pays for persistent skills: auras, buffs and permanent minions. With more reserved than you have, some of them would not run in game, but Path of Building still counts their effect, so your real numbers are lower than shown.".into(),
            how: "Drop a persistent skill or support you can do without, or add Spirit from gear or passives.".into(),
            gain: None,
            tool: None,
            source: Some(SPIRIT),
        };
    }
    if let Some(rest) = t.strip_prefix("You do not meet the ") {
        return Step {
            title: format!(
                "Meet the {}",
                rest.replace(" requirement of ", " requirement for ")
            ),
            why: format!(
                "Path of Building warns: you do not meet the {rest}. In game it would not be usable, but Path of Building still counts it."
            ),
            how: "Add the attribute from gear or passives, or replace what needs it.".into(),
            gain: None,
            tool: Some("item"),
            source: None,
        };
    }
    if let Some(rest) = t.strip_prefix("Not enough ") {
        return Step {
            title: format!(
                "Make sure you can afford {}",
                rest.split(" to use ").nth(1).unwrap_or(rest)
            ),
            why: format!(
                "Not enough {rest}: its cost is more than you have. Path of Building still counts its damage, but in game you could not use it."
            ),
            how: "Raise the resource it costs, or lower its cost with supports or gear.".into(),
            gain: None,
            tool: None,
            source: None,
        };
    }
    Step {
        title: format!("Fix: {t}"),
        why: f.detail.clone().unwrap_or_default(),
        how: "Path of Building's own warning: fix it in the build, then load it here again.".into(),
        gain: None,
        tool: None,
        source: None,
    }
}

/// "Fire resistance 70%, maximum 75%" → a step, with the mechanic in
/// plain numbers.
fn resistance_step(f: &Finding, stats: &Stats<'_>) -> Option<Step> {
    let element = f.title.split(" resistance ").next()?;
    if !["Fire", "Cold", "Lightning", "Chaos"].contains(&element) {
        return None;
    }
    let now = stats.get(&format!("{element}Resist"))?;
    let max = f
        .title
        .rsplit("maximum ")
        .next()?
        .trim_end_matches('%')
        .parse::<f64>()
        .ok()?;
    let add = f
        .fix
        .as_deref()?
        .trim_start_matches('+')
        .split('%')
        .next()?
        .parse::<f64>()
        .ok()?;
    let lower = element.to_lowercase();
    let ratio = (100.0 - now) / (100.0 - max);
    let taken = if ratio >= 1.05 {
        format!(
            " At {}% you take {} the {lower} damage you would at {}%.",
            plain(now),
            times(ratio),
            plain(max)
        )
    } else {
        String::new()
    };
    let rolls = rolls_text(&f.rolls_on, "%");
    if element == "Chaos" {
        return Some(Step {
            title: format!("Add a chaos resistance mod (+{}%)", plain(add)),
            why: format!(
                "Chaos resistance cuts chaos damage the same way, but few mods roll it, so most builds stay below the {}% maximum and add it one mod at a time.{taken}",
                plain(max)
            ),
            how: format!(
                "Look for +{}% or more to Chaos Resistance.{rolls}",
                plain(add)
            ),
            gain: effects(f),
            tool: Some("gear"),
            source: Some(CHAOS),
        });
    }
    Some(Step {
        title: format!("Get {lower} resistance to {}%", plain(max)),
        why: format!(
            "Each point of {lower} resistance cuts {lower} damage you take by 1%, up to your maximum of {}%.{taken} Resistance is usually the cheapest defence to fix, because many item slots can roll it.",
            plain(max)
        ),
        how: format!("You need +{}% {lower} resistance.{rolls}", plain(add)),
        gain: effects(f),
        tool: Some("gear"),
        source: Some(RESISTANCE),
    })
}

/// The damage type whose max hit lags far behind the next one.
fn weak_point_step(stats: &Stats<'_>, steps: &[Step]) -> Option<Step> {
    let hits = stats.max_hits();
    let (weak, next) = (hits.first()?, hits.get(1)?);
    if weak.1 >= next.1 * WEAK_POINT {
        return None;
    }
    let lower = weak.0.to_lowercase();
    if steps.iter().any(|s| {
        s.title
            .to_lowercase()
            .contains(&format!("{lower} resistance"))
    }) {
        return None;
    }
    let (layers, source) = if weak.0 == "Physical" {
        (
            "Armour reduces physical hits (more against many small hits than one big one), evasion lets you avoid hits, and more life or energy shield raises every max hit.",
            Some(ARMOUR),
        )
    } else {
        (
            "Your resistance to it is already at its maximum, so more maximum resistance or larger pools (life, energy shield) are what raise it.",
            Some(if weak.0 == "Chaos" { CHAOS } else { RESISTANCE }),
        )
    };
    Some(Step {
        title: format!("Shore up {lower} hits"),
        why: format!(
            "Your smallest max hit is {lower}: {}, against {} for {}. One {lower} hit bigger than that kills you from full, so it is the hit to fear. {layers}",
            number(weak.1),
            number(next.1),
            next.0.to_lowercase()
        ),
        how: "On Gear upgrades, rank by Survival: for each slot it shows which stats raise your max hits and EHP the most.".into(),
        gain: None,
        tool: Some("gear"),
        source,
    })
}

fn accuracy_step(f: &Finding) -> Step {
    let chance = f
        .title
        .trim_start_matches("Hit chance ")
        .trim_end_matches('%')
        .parse::<f64>()
        .unwrap_or(0.0);
    Step {
        title: format!("Raise your hit chance from {}% to 100%", plain(chance)),
        why: format!(
            "Attacks can miss, and accuracy decides how often. You miss {}% of your hits, and a missed hit deals nothing. Accuracy also falls off with distance to the target (none within 2 metres).",
            plain(100.0 - chance)
        ),
        how: format!(
            "{} from gear or passives.",
            f.fix.as_deref().unwrap_or("More accuracy")
        ),
        gain: effects(f),
        tool: Some("gear"),
        source: Some(ACCURACY),
    }
}

/// "+1 to Level of all Spell Skills" → "spell skills"; "… all Skills" →
/// "all skills".
fn level_kind(line: &str) -> String {
    let kind = line.trim_start_matches("+1 to Level of all ");
    match kind {
        "Skills" => "all skills".into(),
        _ => kind.to_lowercase(),
    }
}

/// The highest roll of a skill-level mod on gear, in PoB's data.
fn top_roll(l: &LevelWorth) -> f64 {
    l.rolls_on
        .iter()
        .filter(|r| category_name(&r.category).is_some())
        .map(|r| r.max)
        .fold(0.0, f64::max)
}

/// The skill-level mod most worth looking for: the gain per level times the
/// most levels one mod can roll (PoB's data), so a mod that only rolls +1
/// does not win over one that rolls several. Gear-rolling mods only.
fn best_level(levels: &[LevelWorth]) -> Option<&LevelWorth> {
    levels
        .iter()
        .filter(|l| l.damage.change >= LEVEL_STEP && top_roll(l) > 0.0)
        .max_by(|a, b| (a.damage.change * top_roll(a)).total_cmp(&(b.damage.change * top_roll(b))))
}

fn level_step(l: &LevelWorth) -> Step {
    let kind = level_kind(&l.line);
    let top = top_roll(l);
    Step {
        title: format!("Look for + levels to {kind}"),
        why: format!(
            "A skill's damage grows with its level. One more level of {kind} is worth {} {} to this build{}.",
            signed(l.damage.change),
            l.damage.label,
            if top > 1.0 {
                format!(", and one mod can roll up to +{} levels", plain(top))
            } else {
                String::new()
            }
        ),
        how: format!(
            "Look for “{}”.{}",
            l.line.replacen("+1", "+#", 1),
            rolls_text(&l.rolls_on, "")
        ),
        gain: Some(format!(
            "{} {} → {} for one level",
            l.damage.label,
            number(l.damage.before),
            number(l.damage.after)
        )),
        tool: Some("item"),
        source: None,
    }
}

fn conditions_step(r: &Realistic) -> Option<Step> {
    let i = r.metrics.iter().position(|m| m.damage)?;
    let loss = -crate::pct(r.configured[i], r.floor[i]);
    if loss < CONDITIONS_STEP {
        return None;
    }
    let mut top: Vec<_> = r
        .conditions
        .iter()
        .filter(|c| c.changes[i] < -0.05)
        .collect();
    top.sort_by(|a, b| a.changes[i].total_cmp(&b.changes[i]));
    let names: Vec<String> = top
        .iter()
        .take(3)
        .map(|c| format!("“{}” ({})", c.condition.label, signed(c.changes[i])))
        .collect();
    Some(Step {
        title: "Check which combat conditions you really keep up".into(),
        why: format!(
            "Path of Building counts {} conditions as always true. Without any of them your damage would be {}, {} less. The ones worth most: {}. If you can't keep one up against a boss, the damage it adds isn't real.",
            r.conditions.len(),
            number(r.floor[i]),
            percent(loss),
            names.join(", ")
        ),
        how: "Open Realistic numbers and tick the ones you sustain; the passive tree plan then uses the same choice.".into(),
        gain: None,
        tool: Some("realistic"),
        source: None,
    })
}

fn damage_points(
    overview: &Overview,
    stats: &Stats<'_>,
    diagnosis: &Diagnosis,
    realistic: Option<&Realistic>,
) -> Vec<Point> {
    let (dmg, label) = stats.damage();
    let mut out = vec![Point {
        title: format!("{} {label}", number(dmg)),
        text: format!(
            "{}{}",
            overview
                .build
                .main_skill
                .as_deref()
                .map(|s| format!("Main skill: {s}. "))
                .unwrap_or_default(),
            if label == "Full DPS" {
                "Full DPS adds up every skill the build marks for it."
            } else {
                "DPS of the main skill, plus minions when the build has them."
            }
        ),
    }];
    let levels: Vec<String> = diagnosis
        .levels
        .iter()
        .filter(|l| l.damage.change > 0.0)
        .take(3)
        .map(|l| format!("{} {}", level_kind(&l.line), signed(l.damage.change)))
        .collect();
    if !levels.is_empty() {
        out.push(Point {
            title: "What scales it".into(),
            text: format!("One more skill level: {}.", levels.join(", ")),
        });
    }
    if let Some(r) = realistic
        && let Some(i) = r.metrics.iter().position(|m| m.damage)
    {
        let mut top: Vec<_> = r
            .conditions
            .iter()
            .filter(|c| c.changes[i] < -0.05)
            .collect();
        top.sort_by(|a, b| a.changes[i].total_cmp(&b.changes[i]));
        if !top.is_empty() {
            out.push(Point {
                title: "What it depends on".into(),
                text: format!(
                    "{}. Each is what that condition adds on its own.",
                    top.iter()
                        .take(3)
                        .map(|c| format!("“{}” {}", c.condition.label, signed(-c.changes[i])))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            });
        }
    }
    out
}

fn defence_points(stats: &Stats<'_>) -> Vec<Point> {
    let mut out = Vec::new();
    let life = stats.get("Life").unwrap_or(0.0);
    let es = stats.get("EnergyShield").unwrap_or(0.0);
    out.push(Point {
        title: if es > life {
            format!("{} energy shield, {} life", number(es), number(life))
        } else if es > 0.0 {
            format!("{} life, {} energy shield", number(life), number(es))
        } else {
            format!("{} life", number(life))
        },
        text: "Your pools: what a hit has to get through. Every max hit below grows with them."
            .into(),
    });
    let hits = stats.max_hits();
    if !hits.is_empty() {
        out.push(Point {
            title: "Biggest hit you survive".into(),
            text: format!(
                "{}. The smallest is your weak point: one hit of that type above it kills you from full.",
                hits.iter()
                    .map(|(t, v)| format!("{} {}", t, number(*v)))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        });
    }
    let res: Vec<String> = ["Fire", "Cold", "Lightning", "Chaos"]
        .iter()
        .filter_map(|e| {
            stats
                .get(&format!("{e}Resist"))
                .map(|v| format!("{} {}%", e.to_lowercase(), plain(v)))
        })
        .collect();
    if !res.is_empty() {
        out.push(Point {
            title: "Resistances".into(),
            text: format!(
                "{}. Each point cuts that element's damage by 1%, up to the maximum (75% unless something raises it).",
                res.join(", ")
            ),
        });
    }
    let armour = stats.get("Armour").unwrap_or(0.0);
    let evasion = stats.get("Evasion").unwrap_or(0.0);
    if armour >= MITIGATION_SHOWN || evasion >= MITIGATION_SHOWN {
        let mut parts = Vec::new();
        if armour >= MITIGATION_SHOWN {
            parts.push(format!(
                "{} armour reduces physical hits, more against many small hits than one big one",
                number(armour)
            ));
        }
        if evasion >= MITIGATION_SHOWN {
            parts.push(format!(
                "{} evasion gives a chance to avoid hits entirely",
                number(evasion)
            ));
        }
        out.push(Point {
            title: "Mitigation".into(),
            text: format!("{}.", capitalise(&parts.join("; "))),
        });
    }
    out
}

/// " It rolls on rings, amulets and belts (up to +45% per mod)." `unit` is
/// "%" for percentages, "" for counts.
fn rolls_text(rolls: &[RollRange], unit: &str) -> String {
    let mut names: Vec<&str> = rolls
        .iter()
        .filter_map(|r| category_name(&r.category))
        .collect();
    names.dedup();
    if names.is_empty() {
        return String::new();
    }
    let top = rolls.iter().map(|r| r.max).fold(0.0, f64::max);
    format!(
        " It rolls on {} (up to +{}{unit} per mod, in Path of Building's data).",
        list(&names),
        plain(top)
    )
}

/// PoB's item categories in plain words; jewels and flasks are left out.
fn category_name(category: &str) -> Option<&'static str> {
    Some(match category {
        "Amulet" => "amulets",
        "Ring" => "rings",
        "Belt" => "belts",
        "Helmet" | "Helmets" => "helmets",
        "Chest" => "body armours",
        "Gloves" => "gloves",
        "Boots" => "boots",
        "Shield" => "shields",
        "Focus" => "foci",
        "Quiver" => "quivers",
        "Wand" => "wands",
        "Staff" => "staves",
        "Sceptre" => "sceptres",
        "Bow" => "bows",
        "Crossbow" => "crossbows",
        "Quarterstaff" => "quarterstaves",
        "Spear" => "spears",
        "Talisman" => "talismans",
        "Flail" => "flails",
        "Claw" => "claws",
        "Dagger" => "daggers",
        "1HMace" => "one-handed maces",
        "2HMace" => "two-handed maces",
        "1HWeapon" => "one-handed weapons",
        "2HWeapon" => "two-handed weapons",
        _ => return None,
    })
}

/// "a", "a and b", "a, b and c".
fn list(items: &[&str]) -> String {
    match items {
        [] => String::new(),
        [one] => (*one).to_owned(),
        [rest @ .., last] => format!("{} and {last}", rest.join(", ")),
    }
}

fn effects(f: &Finding) -> Option<String> {
    (!f.effects.is_empty()).then(|| {
        f.effects
            .iter()
            .map(|e| {
                if e.percent {
                    format!("{} {}% → {}%", e.label, plain(e.before), plain(e.after))
                } else {
                    format!(
                        "{} {} → {} ({})",
                        e.label,
                        number(e.before),
                        number(e.after),
                        signed(e.change)
                    )
                }
            })
            .collect::<Vec<_>>()
            .join(" · ")
    })
}

/// 2.96 → "almost 3 times", 1.4 → "1.4 times".
fn times(ratio: f64) -> String {
    let rounded = ratio.round();
    if (ratio - rounded).abs() <= 0.1 && rounded >= 2.0 {
        let near = if ratio < rounded { "almost " } else { "about " };
        return format!("{near}{rounded:.0} times");
    }
    format!("{ratio:.1} times")
}

/// Thousands separated, no decimals from 1,000 up.
fn number(v: f64) -> String {
    if !v.is_finite() {
        return "–".into();
    }
    if v.abs() < 1000.0 {
        return plain((v * 10.0).round() / 10.0);
    }
    let s = format!("{:.0}", v.abs());
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if v < 0.0 { format!("-{out}") } else { out }
}

/// Whole numbers without decimals, others to one decimal place.
fn plain(v: f64) -> String {
    if v.fract() == 0.0 {
        format!("{v:.0}")
    } else {
        format!("{v:.1}")
    }
}

fn percent(v: f64) -> String {
    format!("{}%", plain((v * 10.0).round() / 10.0))
}

fn signed(change: f64) -> String {
    format!(
        "{}{:.1}%",
        if change >= 0.0 { "+" } else { "−" },
        change.abs()
    )
}

fn capitalise(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn lowercase_first(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_lowercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explains_damage_taken_in_plain_ratios() {
        assert_eq!(times((100.0 - 26.0) / (100.0 - 75.0)), "almost 3 times");
        assert_eq!(times((100.0 - 70.0) / (100.0 - 75.0)), "1.2 times");
        assert_eq!(number(29_492.4), "29,492");
        assert_eq!(number(26.0), "26");
        assert_eq!(
            list(&["rings", "amulets", "belts"]),
            "rings, amulets and belts"
        );
    }

    #[test]
    fn suggests_the_level_mod_with_the_most_reachable_value() {
        use crate::diagnose::Effect;
        let level = |line: &str, change: f64, top: f64| LevelWorth {
            line: line.into(),
            damage: Effect {
                label: "DPS".into(),
                before: 100.0,
                after: 100.0 + change,
                change,
                percent: false,
            },
            new_warnings: Vec::new(),
            rolls_on: vec![RollRange {
                category: "Wand".into(),
                min: 1.0,
                max: top,
            }],
        };
        // +15.7% but only +1 per mod loses to +15.2% that rolls up to +7.
        let levels = vec![
            level("+1 to Level of all Skills", 15.7, 1.0),
            level("+1 to Level of all Lightning Spell Skills", 15.2, 7.0),
            level("+1 to Level of all Fire Spell Skills", 0.1, 7.0),
        ];
        assert_eq!(
            best_level(&levels).unwrap().line,
            "+1 to Level of all Lightning Spell Skills"
        );
        assert_eq!(level_kind("+1 to Level of all Skills"), "all skills");
    }

    #[test]
    fn names_where_a_stat_rolls() {
        let rolls = vec![
            RollRange {
                category: "Amulet".into(),
                min: 6.0,
                max: 45.0,
            },
            RollRange {
                category: "AnyJewel".into(),
                min: 5.0,
                max: 10.0,
            },
            RollRange {
                category: "Ring".into(),
                min: 6.0,
                max: 45.0,
            },
        ];
        assert_eq!(
            rolls_text(&rolls, "%"),
            " It rolls on amulets and rings (up to +45% per mod, in Path of Building's data)."
        );
    }
}
