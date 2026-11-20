//! Path of Building PoE2, headless.
//!
//! This crate runs PoB2's own Lua calculation code on the exact LuaJIT build
//! the PoB2 app ships. It never reimplements game maths: every number it
//! returns is a number PoB computed.
//!
//! - [`Install`]: a verified copy of one PoB2 release on disk.
//! - [`Engine`]: one sandboxed Lua state with PoB loaded; loads builds and
//!   returns PoB's settled output.
//! - [`StatSheet`]: the stats PoB writes into a build file, compared exactly.
//! - [`code`]: PoB build codes (`base64url(zlib(xml))`).

pub mod code;
pub mod config_xml;
mod data_dir;
mod engine;
mod error;
mod gear;
pub mod input;
mod install;
mod item;
mod pool;
mod stats;
mod tree;

/// The PoB2 release the doctor calculates with: the current PoB2 app release.
/// Bump it, then run `doctor verify`, when PoB2 ships an update.
pub const PINNED_VERSION: &str = "0.23.1";

pub use data_dir::{DataDir, version_key};
pub use engine::{
    Assumption, AssumptionKind, Calculation, Engine, Identity, Outputs, SkillUse, UnsupportedMod,
};
pub use error::{Error, Result};
pub use gear::{GearSlot, StatValue, StatValues};
pub use install::{Install, InstallProgress, PobVersion};
pub use item::{ItemInfo, ItemSlot};
pub use pool::Pool;
pub use stats::{Stat, StatDiff, StatKind, StatSheet};
pub use tree::{NodeRef, Summary, TreeOption, TreeScreen, TreeTrial};
