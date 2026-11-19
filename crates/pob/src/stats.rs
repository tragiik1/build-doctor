//! The stats PoB writes into a build file when it saves.
//!
//! PoB stores each value as Lua's `tostring(number)`, so two sheets are
//! compared as strings: equal means bit-for-bit the same calculation result
//! at PoB's 14 significant digits, not "close enough".

use std::fmt;

use crate::{Error, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum StatKind {
    /// `<PlayerStat>`: sidebar stats of the player.
    Player,
    /// `<MinionStat>`: sidebar stats of the active minion.
    Minion,
    /// `<FullDPSSkill>`: per-skill rows of PoB's Full DPS breakdown.
    FullDps,
}

impl fmt::Display for StatKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Player => "player",
            Self::Minion => "minion",
            Self::FullDps => "full-dps",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stat {
    pub kind: StatKind,
    /// PoB's output key, e.g. `TotalDPS` or `FireMaximumHitTaken`. For Full
    /// DPS rows: the skill label, plus `@skillPart` when PoB records one.
    pub name: String,
    /// Exactly as PoB wrote it.
    pub value: String,
}

impl Stat {
    /// The value as a number, if PoB wrote a number.
    pub fn number(&self) -> Option<f64> {
        self.value.parse().ok()
    }
}

/// All stats from one saved build, in the order PoB wrote them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StatSheet {
    stats: Vec<Stat>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatDiff {
    /// Present in both, different values.
    Changed {
        kind: StatKind,
        name: String,
        expected: String,
        actual: String,
    },
    /// Expected but not produced.
    Missing {
        kind: StatKind,
        name: String,
        expected: String,
    },
    /// Produced but not expected (newer PoB versions save more stats).
    Extra {
        kind: StatKind,
        name: String,
        actual: String,
    },
}

impl StatDiff {
    /// Whether this difference means the calculation disagrees. `Extra` stats
    /// are not a disagreement: the reference simply did not record them.
    pub fn is_mismatch(&self) -> bool {
        !matches!(self, Self::Extra { .. })
    }
}

impl fmt::Display for StatDiff {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Changed {
                kind,
                name,
                expected,
                actual,
            } => {
                write!(f, "{kind} {name}: expected {expected}, got {actual}")
            }
            Self::Missing {
                kind,
                name,
                expected,
            } => {
                write!(f, "{kind} {name}: expected {expected}, not produced")
            }
            Self::Extra { kind, name, actual } => {
                write!(f, "{kind} {name}: produced {actual}, not in reference")
            }
        }
    }
}

impl StatSheet {
    /// Reads the stats from PoB build XML.
    pub fn from_build_xml(xml: &str) -> Result<Self> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| Error::Xml(e.to_string()))?;
        let build = doc
            .root_element()
            .children()
            .find(|n| n.has_tag_name("Build"))
            .ok_or_else(|| Error::Xml("no <Build> element".into()))?;

        let mut stats = Vec::new();
        for node in build.children().filter(roxmltree::Node::is_element) {
            let kind = match node.tag_name().name() {
                "PlayerStat" => StatKind::Player,
                "MinionStat" => StatKind::Minion,
                "FullDPSSkill" => StatKind::FullDps,
                _ => continue,
            };
            let attr = |name: &str| {
                node.attribute(name).ok_or_else(|| {
                    Error::Xml(format!("<{}> without {name}", node.tag_name().name()))
                })
            };
            let mut name = attr("stat")?.to_owned();
            if kind == StatKind::FullDps
                && let Some(part) = node.attribute("skillPart").filter(|p| !p.is_empty())
            {
                name = format!("{name}@{part}");
            }
            stats.push(Stat {
                kind,
                name,
                value: attr("value")?.to_owned(),
            });
        }
        Ok(Self { stats })
    }

    pub fn iter(&self) -> impl Iterator<Item = &Stat> {
        self.stats.iter()
    }

    pub fn len(&self) -> usize {
        self.stats.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stats.is_empty()
    }

    /// The first stat of `kind` called `name`.
    pub fn get(&self, kind: StatKind, name: &str) -> Option<&Stat> {
        self.stats.iter().find(|s| s.kind == kind && s.name == name)
    }

    /// Shorthand for a numeric player stat.
    pub fn player(&self, name: &str) -> Option<f64> {
        self.get(StatKind::Player, name).and_then(Stat::number)
    }

    /// Every difference between `self` (the reference) and `actual`.
    ///
    /// Full DPS rows can legitimately repeat a label, so stats are matched by
    /// (kind, name, occurrence) rather than by name alone.
    pub fn diff(&self, actual: &StatSheet) -> Vec<StatDiff> {
        let expected = keyed(&self.stats);
        let actual = keyed(&actual.stats);
        let mut diffs = Vec::new();

        for (key, e) in &expected {
            match actual.iter().find(|(k, _)| k == key) {
                Some((_, a)) if a.value == e.value => {}
                Some((_, a)) => diffs.push(StatDiff::Changed {
                    kind: e.kind,
                    name: e.name.clone(),
                    expected: e.value.clone(),
                    actual: a.value.clone(),
                }),
                None => diffs.push(StatDiff::Missing {
                    kind: e.kind,
                    name: e.name.clone(),
                    expected: e.value.clone(),
                }),
            }
        }
        for (key, a) in &actual {
            if !expected.iter().any(|(k, _)| k == key) {
                diffs.push(StatDiff::Extra {
                    kind: a.kind,
                    name: a.name.clone(),
                    actual: a.value.clone(),
                });
            }
        }
        diffs
    }
}

type Key<'a> = (StatKind, &'a str, usize);

fn keyed(stats: &[Stat]) -> Vec<(Key<'_>, &Stat)> {
    let mut out: Vec<(Key<'_>, &Stat)> = Vec::with_capacity(stats.len());
    for stat in stats {
        let occurrence = out
            .iter()
            .filter(|((k, n, _), _)| *k == stat.kind && *n == stat.name)
            .count();
        out.push(((stat.kind, stat.name.as_str(), occurrence), stat));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const XML: &str = r#"<PathOfBuilding2><Build level="90">
        <PlayerStat stat="TotalDPS" value="1234.5"/>
        <PlayerStat value="5000" stat="Life"/>
        <MinionStat stat="TotalDPS" value="99"/>
        <FullDPSSkill stat="Spark" value="10" skillPart="" source=""/>
        <FullDPSSkill stat="Spark" value="20" skillPart="2" source=""/>
    </Build></PathOfBuilding2>"#;

    #[test]
    fn parses_all_kinds_regardless_of_attribute_order() {
        let sheet = StatSheet::from_build_xml(XML).unwrap();
        assert_eq!(sheet.len(), 5);
        assert_eq!(sheet.player("Life"), Some(5000.0));
        assert_eq!(sheet.get(StatKind::Minion, "TotalDPS").unwrap().value, "99");
        assert!(sheet.get(StatKind::FullDps, "Spark@2").is_some());
    }

    #[test]
    fn identical_sheets_have_no_diff() {
        let sheet = StatSheet::from_build_xml(XML).unwrap();
        assert!(sheet.diff(&sheet).is_empty());
    }

    #[test]
    fn reports_changed_missing_and_extra() {
        let expected = StatSheet::from_build_xml(XML).unwrap();
        let actual = StatSheet::from_build_xml(
            r#"<PathOfBuilding2><Build>
                <PlayerStat stat="TotalDPS" value="1234.50001"/>
                <MinionStat stat="TotalDPS" value="99"/>
                <FullDPSSkill stat="Spark" value="10" skillPart=""/>
                <FullDPSSkill stat="Spark" value="20" skillPart="2"/>
                <PlayerStat stat="Mana" value="10"/>
            </Build></PathOfBuilding2>"#,
        )
        .unwrap();
        let diffs = expected.diff(&actual);
        assert_eq!(diffs.len(), 3);
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, StatDiff::Changed { name, .. } if name == "TotalDPS"))
        );
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, StatDiff::Missing { name, .. } if name == "Life"))
        );
        assert!(
            diffs
                .iter()
                .any(|d| matches!(d, StatDiff::Extra { name, .. } if name == "Mana"))
        );
        assert_eq!(diffs.iter().filter(|d| d.is_mismatch()).count(), 2);
    }

    #[test]
    fn values_compare_as_written_not_as_numbers() {
        let a =
            StatSheet::from_build_xml(r#"<P><Build><PlayerStat stat="X" value="1"/></Build></P>"#)
                .unwrap();
        let b = StatSheet::from_build_xml(
            r#"<P><Build><PlayerStat stat="X" value="1.0"/></Build></P>"#,
        )
        .unwrap();
        assert_eq!(a.diff(&b).len(), 1);
    }
}
