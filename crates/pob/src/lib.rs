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
mod engine;
mod error;
mod install;
mod pool;
mod stats;
mod tree;

pub use engine::{Assumption, AssumptionKind, Calculation, Engine, Identity, UnsupportedMod};
pub use error::{Error, Result};
pub use install::{Install, PobVersion};
pub use pool::Pool;
pub use stats::{Stat, StatDiff, StatKind, StatSheet};
pub use tree::{NodeRef, Summary, TreeOption, TreeScreen, TreeTrial};
