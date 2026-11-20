//! What each gear stat is worth to a build, slot by slot, measured the way
//! PoB's own trade search measures it.

use mlua::Table;

use crate::{Engine, Result, Summary};

/// A worn item a trade search can replace.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GearSlot {
    pub slot: String,
    pub item: String,
    /// The item's base ("Slipstrike Vest", …).
    pub base: String,
    pub rarity: String,
    /// PoB's item category for roll ranges ("Ring", "Helmet", …).
    pub category: String,
    /// The trade site's category filter ("accessory.ring", …).
    pub trade_category: String,
}

/// One trade stat on a blank item in the slot, at a mid roll.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatValue {
    /// The trade site's stat id ("explicit.stat_3299347043", …).
    pub trade_id: String,
    /// The mod line PoB evaluated ("+85 to maximum Life").
    pub line: String,
    /// The value in that line (the mid roll).
    pub value: f64,
    pub with_stat: Summary,
}

#[derive(Debug, Clone)]
pub struct StatValues {
    /// The build as it is.
    pub current: Summary,
    /// The worn item replaced by a blank rare of the same base.
    pub blank: Summary,
    pub stats: Vec<StatValue>,
}

impl Engine {
    /// Worn items a trade search can replace (not weapon swap, flasks or
    /// charms).
    pub fn gear_slots(&self) -> Result<Vec<GearSlot>> {
        let rows: Vec<Table> = self.call("bd_gear_slots", ())?;
        rows.into_iter()
            .map(|r| {
                Ok(GearSlot {
                    slot: r.get("slot")?,
                    item: r.get("item")?,
                    base: r.get("base")?,
                    rarity: r.get("rarity")?,
                    category: r.get("category")?,
                    trade_category: r.get("tradeCategory")?,
                })
            })
            .collect()
    }

    /// Every `parts`-th trade stat that can roll in `slot`, starting at
    /// `part`, each measured alone at a mid roll on a blank rare of the worn
    /// item's base, with PoB's calculator. The build is not changed.
    pub fn stat_values(&self, slot: &str, part: usize, parts: usize) -> Result<StatValues> {
        let t: Table = self.call("bd_stat_values", (slot, part, parts.max(1)))?;
        let stats: Vec<Table> = t.get("stats")?;
        Ok(StatValues {
            current: Summary::from_lua(&t.get("current")?)?,
            blank: Summary::from_lua(&t.get("blank")?)?,
            stats: stats
                .into_iter()
                .map(|s| {
                    Ok(StatValue {
                        trade_id: s.get("id")?,
                        line: s.get("line")?,
                        value: s.get("value")?,
                        with_stat: Summary::from_lua(&s.get("out")?)?,
                    })
                })
                .collect::<Result<_>>()?,
        })
    }
}
