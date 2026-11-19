//! Passive tree what-ifs.
//!
//! Two speeds. Screening uses PoB's own node calculator (the one behind its
//! power report), which reuses cached skill results: fast, good for ordering
//! candidates, not for reporting. Trials apply a change exactly as clicking
//! in PoB would and let PoB recalculate fully: these are the numbers to show.

use mlua::Table;

use crate::{Calculation, Engine, Error, Result, StatSheet};

/// The numbers a ranking needs, from one calculation.
#[derive(Debug, Clone, Copy, PartialEq, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub full_dps: f64,
    pub combined_dps: f64,
    pub minion_dps: f64,
    pub ehp: f64,
    /// Smallest of the five "maximum hit taken" values.
    pub min_hit: f64,
}

impl Summary {
    /// The same numbers read from a full calculation's saved stats.
    pub fn from_stats(stats: &StatSheet) -> Self {
        let player = |name: &str| stats.player(name).unwrap_or(0.0);
        let min_hit = ["Physical", "Fire", "Cold", "Lightning", "Chaos"]
            .iter()
            .filter_map(|t| stats.player(&format!("{t}MaximumHitTaken")))
            .reduce(f64::min)
            .unwrap_or(0.0);
        Self {
            full_dps: player("FullDPS"),
            combined_dps: player("CombinedDPS"),
            minion_dps: stats
                .get(crate::StatKind::Minion, "CombinedDPS")
                .and_then(crate::Stat::number)
                .unwrap_or(0.0),
            ehp: player("TotalEHP"),
            min_hit,
        }
    }

    fn from_lua(table: &Table) -> Result<Self> {
        Ok(Self {
            full_dps: table.get("fullDps")?,
            combined_dps: table.get("combinedDps")?,
            minion_dps: table.get("minionDps")?,
            ehp: table.get("ehp")?,
            min_hit: table.get("minHit")?,
        })
    }
}

/// A passive node, by PoB's id and display name.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize)]
pub struct NodeRef {
    pub id: u32,
    pub name: String,
    /// PoB's node type: "Normal", "Notable", "Keystone", …
    pub kind: String,
    /// The node's stat lines as PoB shows them.
    pub stats: Vec<String>,
}

/// One screened tree option: allocating a node along its path, or removing
/// an allocated node together with the nodes that depend on it.
#[derive(Debug, Clone)]
pub struct TreeOption {
    pub node: NodeRef,
    /// Every node allocated (path) or removed (dependents), target included.
    pub nodes: Vec<u32>,
    /// Passive points this costs (adds) or frees (removes).
    pub cost: u32,
    /// Screened result (approximate; see module docs).
    pub screened: Summary,
}

#[derive(Debug, Clone)]
pub struct TreeScreen {
    pub base: Summary,
    pub adds: Vec<TreeOption>,
    pub removes: Vec<TreeOption>,
}

/// An exact tree change: what PoB actually allocated and removed, and its
/// full recalculation.
#[derive(Debug, Clone)]
pub struct TreeTrial {
    pub calc: Calculation,
    pub added: Vec<NodeRef>,
    pub removed: Vec<NodeRef>,
    pub points_before: u32,
    pub points_after: u32,
    /// PoB warnings this change introduces (unmet requirements, …).
    pub new_warnings: Vec<String>,
}

impl Engine {
    /// Screens every unallocated node within `max_distance` points and every
    /// allocated node (with its dependents). `part` of `parts` splits the
    /// work between engines; one engine screens everything with `0, 1`.
    pub fn tree_screen(&self, max_distance: u32, part: usize, parts: usize) -> Result<TreeScreen> {
        let result: Table = self.call("bd_tree_screen", (max_distance, part, parts.max(1)))?;
        let options = |key: &str| -> Result<Vec<TreeOption>> {
            result
                .get::<Vec<Table>>(key)?
                .iter()
                .map(|t| {
                    Ok(TreeOption {
                        node: node_ref(&t.get("node")?)?,
                        nodes: t.get("nodes")?,
                        cost: t.get("cost")?,
                        screened: Summary::from_lua(&t.get("out")?)?,
                    })
                })
                .collect()
        };
        Ok(TreeScreen {
            base: Summary::from_lua(&result.get("base")?)?,
            adds: options("adds")?,
            removes: options("removes")?,
        })
    }

    /// Screens one combined change (approximate).
    pub fn tree_screen_change(&self, remove: &[u32], add: &[u32]) -> Result<Summary> {
        let out: Table = self.call("bd_tree_screen_change", (remove.to_vec(), add.to_vec()))?;
        Summary::from_lua(&out)
    }

    /// Applies a tree change as clicking in PoB would, recalculates fully,
    /// and restores the tree.
    pub fn try_tree(&self, remove: &[u32], add: &[u32]) -> Result<TreeTrial> {
        let t: Table = self.call("bd_try_tree", (remove.to_vec(), add.to_vec()))?;
        let xml: Option<String> = t.get("xml")?;
        let passes: u32 = t.get("passes")?;
        let xml = xml.ok_or(Error::Unsettled {
            passes,
            changing: Vec::new(),
        })?;
        let stats = StatSheet::from_build_xml(&xml)?;
        let refs = |key: &str| -> Result<Vec<NodeRef>> {
            t.get::<Vec<Table>>(key)?.iter().map(node_ref).collect()
        };
        Ok(TreeTrial {
            calc: Calculation { xml, stats, passes },
            added: refs("added")?,
            removed: refs("removed")?,
            points_before: t.get("pointsBefore")?,
            points_after: t.get("pointsAfter")?,
            new_warnings: t.get("newWarnings")?,
        })
    }

    /// PoB's warnings for the loaded build.
    pub fn warnings(&self) -> Result<Vec<String>> {
        self.call("bd_warnings", ())
    }

    pub(crate) fn call<A: mlua::IntoLuaMulti, R: mlua::FromLuaMulti>(
        &self,
        name: &str,
        args: A,
    ) -> Result<R> {
        self.lua
            .globals()
            .get::<mlua::Function>(name)?
            .call::<R>(args)
            .map_err(|err| Error::Pob(self.describe(&err, false)))
    }
}

fn node_ref(t: &Table) -> Result<NodeRef> {
    Ok(NodeRef {
        id: t.get("id")?,
        name: t.get::<Option<String>>("name")?.unwrap_or_default(),
        kind: t.get::<Option<String>>("kind")?.unwrap_or_default(),
        stats: t.get::<Option<Vec<String>>>("stats")?.unwrap_or_default(),
    })
}
