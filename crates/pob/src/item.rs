//! Item what-ifs: an item from its text, equipped in place of what the build
//! wears, with PoB recalculating fully.

use mlua::Table;

use crate::{Calculation, Engine, Error, Result, StatSheet};

/// What PoB makes of an item's text.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemInfo {
    pub name: String,
    pub rarity: String,
    /// Slots it fits: shown equipment slots (not weapon swap) and allocated
    /// passive tree jewel sockets.
    pub slots: Vec<ItemSlot>,
    /// Lines PoB cannot read; no number includes them.
    pub unread: Vec<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemSlot {
    /// PoB's slot name ("Ring 1", "Jewel 26725").
    pub slot: String,
    /// What to call it ("Ring 1", "Jewel socket 2").
    pub label: String,
    /// What the build has there now.
    pub current: Option<String>,
}

impl Engine {
    /// Reads an item's text (as the game copies it with Ctrl+C) exactly as
    /// given; quality and rolls are not changed.
    pub fn item_info(&self, raw: &str) -> Result<ItemInfo> {
        let info: Table = self.call("bd_item_info", raw)?;
        let slots: Vec<Table> = info.get("slots")?;
        Ok(ItemInfo {
            name: info.get("name")?,
            rarity: info.get("rarity")?,
            slots: slots
                .into_iter()
                .map(|s| {
                    Ok(ItemSlot {
                        slot: s.get("slot")?,
                        label: s.get("label")?,
                        current: s.get("current")?,
                    })
                })
                .collect::<Result<_>>()?,
            unread: info.get("unread")?,
        })
    }

    /// The build with the item in `slot` instead of what is there: PoB's
    /// settled result and its warnings. The build is restored afterwards and
    /// checked byte-identical before anything else is read.
    pub fn with_item(&self, raw: &str, slot: &str) -> Result<(Calculation, Vec<String>)> {
        let (saved, passes, warnings): (Option<String>, u32, Option<Vec<String>>) =
            self.call("bd_with_item", (raw, slot))?;
        let (Some(xml), Some(warnings)) = (saved, warnings) else {
            return Err(Error::Unsettled {
                passes,
                changing: Vec::new(),
            });
        };
        let stats = StatSheet::from_build_xml(&xml)?;
        Ok((Calculation { xml, stats, passes }, warnings))
    }
}
