//! One sandboxed Lua state running PoB2.

use std::ffi::c_int;
use std::path::{self, Path};

use mlua::{Lua, Table};

use crate::{Error, Install, Result, StatSheet};

const BRIDGE: &str = include_str!("../lua/bridge.lua");

unsafe extern "C-unwind" {
    /// From the statically linked `lua-utf8` C module (vendor/luautf8).
    fn luaopen_utf8(state: *mut mlua::ffi::lua_State) -> c_int;
}

/// PoB2 loaded and ready to calculate builds.
///
/// Startup takes one to three seconds (PoB parses its tree and item data), so
/// keep an engine alive and reuse it. An engine is tied to the thread that
/// created it; run one per thread for parallel work.
#[derive(Debug)]
pub struct Engine {
    lua: Lua,
    install: Install,
}

/// Who a build is. Every field is `None` when PoB has no value for it.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub pob_version: Option<String>,
    pub class: Option<String>,
    pub ascendancy: Option<String>,
    pub level: Option<u32>,
    pub tree_version: Option<String>,
    pub main_skill: Option<String>,
}

/// PoB's settled result for one build.
#[derive(Debug, Clone)]
pub struct Calculation {
    /// The build exactly as PoB saves it after calculating, stats included.
    pub xml: String,
    /// The stats PoB wrote into `xml`.
    pub stats: StatSheet,
    /// Recalculations PoB needed before its output stopped changing.
    pub passes: u32,
}

impl Engine {
    pub fn start(install: &Install) -> Result<Self> {
        let lua = Lua::new();

        // SAFETY: luaopen_utf8 is a standard `lua_CFunction` compiled against
        // the same LuaJIT headers this state was built from.
        let open_utf8 = unsafe { lua.create_c_function(luaopen_utf8) }?;
        let package: Table = lua.globals().get("package")?;
        let preload: Table = package.get("preload")?;
        preload.set("lua-utf8", open_utf8)?;

        let src = dir_for_lua(&install.src_dir())?;
        let lib = dir_for_lua(&install.lua_lib_dir())?;
        let engine = Self {
            lua,
            install: install.clone(),
        };

        let started = engine
            .lua
            .load(BRIDGE)
            .set_name("=bridge.lua")
            .call::<()>((src, lib));
        if let Err(err) = started {
            return Err(Error::Startup(engine.describe(&err)));
        }
        Ok(engine)
    }

    pub fn install(&self) -> &Install {
        &self.install
    }

    /// Loads build XML, lets PoB calculate until its output is stable, and
    /// returns what PoB saved.
    pub fn load(&self, xml: &str) -> Result<Calculation> {
        let load: mlua::Function = self.lua.globals().get("bd_load")?;
        let (saved, passes, before, after) = load
            .call::<(Option<String>, u32, Option<String>, Option<String>)>(xml)
            .map_err(|err| Error::Pob(self.describe(&err)))?;
        let Some(xml) = saved else {
            let changing = changed_lines(
                before.as_deref().unwrap_or(""),
                after.as_deref().unwrap_or(""),
            );
            return Err(Error::Unsettled { passes, changing });
        };
        let stats = StatSheet::from_build_xml(&xml)?;
        Ok(Calculation { xml, stats, passes })
    }

    /// The loaded build's class, level and main skill, as PoB resolved them.
    pub fn identity(&self) -> Result<Identity> {
        let identity: Table = self
            .lua
            .globals()
            .get::<mlua::Function>("bd_identity")?
            .call(())?;
        Ok(Identity {
            pob_version: identity.get("pobVersion")?,
            class: identity.get("className")?,
            ascendancy: identity.get("ascendancy")?,
            level: identity.get("level")?,
            tree_version: identity.get("treeVersion")?,
            main_skill: identity.get("mainSkill")?,
        })
    }

    /// Lines PoB printed since the last call.
    pub fn take_log(&self) -> Vec<String> {
        self.lua
            .globals()
            .get::<mlua::Function>("bd_take_log")
            .and_then(|f| f.call::<Vec<String>>(()))
            .unwrap_or_default()
    }

    /// A Lua error plus the tail of PoB's own output, which usually names the
    /// real cause.
    fn describe(&self, err: &mlua::Error) -> String {
        let message = match err {
            mlua::Error::RuntimeError(msg) => msg.clone(),
            mlua::Error::CallbackError { cause, .. } => cause.to_string(),
            other => other.to_string(),
        };
        let log = self.take_log();
        let tail: Vec<&str> = log.iter().rev().take(5).rev().map(String::as_str).collect();
        if tail.is_empty() {
            message
        } else {
            format!("{message}\nPoB output:\n  {}", tail.join("\n  "))
        }
    }
}

/// Up to five lines that differ between two saves, as `before -> after`.
fn changed_lines(before: &str, after: &str) -> Vec<String> {
    before
        .lines()
        .zip(after.lines())
        .filter(|(a, b)| a != b)
        .take(5)
        .map(|(a, b)| format!("{} -> {}", a.trim(), b.trim()))
        .collect()
}

/// An absolute directory path ending in a separator, as the bridge expects.
fn dir_for_lua(dir: &Path) -> Result<String> {
    let absolute = path::absolute(dir).map_err(|e| Error::io("resolve", dir, e))?;
    let mut text = absolute
        .to_str()
        .ok_or_else(|| Error::NotAnInstall(format!("{} is not valid UTF-8", absolute.display())))?
        .to_owned();
    if !text.ends_with(['/', '\\']) {
        text.push(path::MAIN_SEPARATOR);
    }
    Ok(text)
}

/// These need a PoB2 install: set `DOCTOR_POB_DIR` to one (a directory with
/// `manifest.xml`, `src/` and `runtime/lua/`) and run with `--include-ignored`.
#[cfg(test)]
mod tests {
    use super::*;

    fn engine() -> Engine {
        let dir =
            std::env::var("DOCTOR_POB_DIR").expect("DOCTOR_POB_DIR must point at a PoB2 install");
        Engine::start(&Install::open(dir).unwrap()).unwrap()
    }

    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn sandbox_blocks_side_effects() {
        let engine = engine();
        let probe = r#"
            local f = io.open("doctor-sandbox-probe.txt", "w")
            local g = io.open("doctor-sandbox-probe.txt", "a+")
            local exec = os.execute("echo hi")
            local pipe = io.popen("echo hi")
            local env = os.getenv("PATH")
            local removed = os.remove("Launch.lua")
            return f == nil and g == nil and exec == nil and pipe == nil and env == nil and removed == nil
        "#;
        assert!(engine.lua.load(probe).eval::<bool>().unwrap());
        assert!(
            !engine
                .install
                .src_dir()
                .join("doctor-sandbox-probe.txt")
                .exists()
        );
    }

    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn sandbox_still_reads_pob_files() {
        let engine = engine();
        let readable = engine
            .lua
            .load(r#"local f = io.open("Launch.lua", "r"); local ok = f ~= nil; if f then f:close() end; return ok"#)
            .eval::<bool>()
            .unwrap();
        assert!(readable);
    }

    /// LuaJIT seeds string hashing per state, so `pairs()` order differs
    /// between engines. PoB's numbers must not depend on it: fresh engines
    /// have to produce identical stats for real builds. (The saved XML may
    /// list attributes in a different order, so it is not compared raw.)
    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn fresh_engines_agree_exactly() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/builds");
        for name in ["maxroll_gd79010q.xml", "7occvtBuTEOp.xml"] {
            let xml = std::fs::read_to_string(dir.join(name)).unwrap();
            let first = engine().load(&xml).unwrap();
            assert!(!first.stats.is_empty());
            for _ in 0..4 {
                let again = engine().load(&xml).unwrap();
                assert_eq!(first.stats.diff(&again.stats), [], "{name}");
            }
        }
    }

    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn rejects_xml_pob_cannot_parse() {
        let err = engine().load("<NotPoB/>").unwrap_err();
        assert!(matches!(err, Error::Pob(_)), "got {err:?}");
    }
}
