//! One sandboxed Lua state running PoB2.

use std::collections::BTreeMap;
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
    pub(crate) lua: Lua,
    pub(crate) install: Install,
}

/// Who a build is. Every field is `None` when PoB has no value for it.
#[derive(Debug, Clone, Default)]
pub struct Identity {
    pub pob_version: Option<String>,
    pub class: Option<String>,
    pub ascendancy: Option<String>,
    pub level: Option<u32>,
    pub tree_version: Option<String>,
    /// The newest passive tree this PoB release knows.
    pub latest_tree_version: Option<String>,
    pub main_skill: Option<String>,
}

impl Identity {
    /// Whether the build uses an older passive tree than this PoB release's
    /// newest one. PoB then calculates on the old tree, so the numbers
    /// describe an older game version.
    pub fn tree_is_outdated(&self) -> bool {
        matches!((&self.tree_version, &self.latest_tree_version), (Some(t), Some(l)) if t != l)
    }
}

/// An item mod line PoB cannot parse, so no PoB number includes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnsupportedMod {
    pub slot: String,
    pub item: String,
    pub line: String,
}

/// One configuration option a build's numbers depend on, as PoB's config
/// tab shows it (for example "Is the enemy Shocked?" = yes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assumption {
    /// PoB's config variable name, e.g. `conditionEnemyShocked`.
    pub var: String,
    pub kind: AssumptionKind,
    pub section: String,
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AssumptionKind {
    /// A state the player has to keep up in a fight (enemy Shocked, on full
    /// life, a custom modifier, …).
    Condition,
    /// What fight is simulated and how PoB calculates it: enemy stats, the
    /// General section's calculation modes, campaign rewards.
    Scenario,
}

/// Numbers from PoB's output table, by PoB's key.
pub type Outputs = BTreeMap<String, f64>;

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
            return Err(Error::Startup(engine.describe(&err, true)));
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
            .map_err(|err| Error::Pob(self.describe(&err, false)))?;
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

    /// Recalculates the loaded build with the given config options unset (as
    /// if unticked or cleared in PoB's config tab) and returns PoB's settled
    /// result. The build is restored afterwards and PoB's output is checked to
    /// be byte-identical to the original, so later calls are unaffected.
    pub fn without_config(&self, vars: &[&str]) -> Result<Calculation> {
        let without: mlua::Function = self.lua.globals().get("bd_without_config")?;
        let (saved, passes) = without
            .call::<(Option<String>, u32)>(vars.to_vec())
            .map_err(|err| Error::Pob(self.describe(&err, false)))?;
        let xml = saved.ok_or(Error::Unsettled {
            passes,
            changing: Vec::new(),
        })?;
        let stats = StatSheet::from_build_xml(&xml)?;
        Ok(Calculation { xml, stats, passes })
    }

    /// Numbers from PoB's output for the loaded build, including ones PoB
    /// does not save (such as `MissingFireResist`). Keys PoB did not set are
    /// absent; booleans read as 1 or 0.
    pub fn outputs(&self, keys: &[&str]) -> Result<Outputs> {
        let values: Table = self
            .lua
            .globals()
            .get::<mlua::Function>("bd_outputs")?
            .call(keys.to_vec())
            .map_err(|err| Error::Pob(self.describe(&err, false)))?;
        read_outputs(values)
    }

    /// One recalculation with extra modifier lines, returning only the
    /// requested outputs: a quick probe for searches. PoB can need more than
    /// one pass to settle, so confirm what a probe finds with
    /// [`Engine::with_mods`].
    pub fn probe_mods(&self, lines: &[&str], keys: &[&str]) -> Result<Outputs> {
        let values: Table = self
            .lua
            .globals()
            .get::<mlua::Function>("bd_probe_mods")?
            .call((lines.to_vec(), keys.to_vec()))
            .map_err(|err| Error::Pob(self.describe(&err, false)))?;
        read_outputs(values)
    }

    /// The least Spirit any skill or support in PoB's data reserves.
    pub fn cheapest_spirit(&self) -> Result<Option<f64>> {
        Ok(self
            .lua
            .globals()
            .get::<mlua::Function>("bd_cheapest_spirit")?
            .call(())?)
    }

    /// Recalculates the loaded build with extra modifier lines, as if typed
    /// into PoB's Custom Modifiers box, and returns PoB's settled result and
    /// the requested outputs. A line PoB cannot read completely is an error.
    /// The build is restored afterwards, as with [`Engine::without_config`].
    pub fn with_mods(&self, lines: &[&str], keys: &[&str]) -> Result<(Calculation, Outputs)> {
        let with: mlua::Function = self.lua.globals().get("bd_with_mods")?;
        let (saved, passes, values) = with
            .call::<(Option<String>, u32, Option<Table>)>((lines.to_vec(), keys.to_vec()))
            .map_err(|err| Error::Pob(self.describe(&err, false)))?;
        let (Some(xml), Some(values)) = (saved, values) else {
            return Err(Error::Unsettled {
                passes,
                changing: Vec::new(),
            });
        };
        let stats = StatSheet::from_build_xml(&xml)?;
        Ok((Calculation { xml, stats, passes }, read_outputs(values)?))
    }

    /// Puts PoB back on the loaded build after what-ifs (recalculating it) and
    /// returns its saved output, checked byte-identical to the original load.
    pub fn ensure_original(&self) -> Result<String> {
        self.lua
            .globals()
            .get::<mlua::Function>("bd_ensure_original")?
            .call::<()>(())
            .map_err(|err| Error::Pob(self.describe(&err, false)))?;
        let saved: Option<String> = self.lua.load("return bd_loaded_xml()").eval()?;
        saved.ok_or_else(|| Error::Pob("no build loaded".into()))
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
            latest_tree_version: identity.get("latestTreeVersion")?,
            main_skill: identity.get("mainSkill")?,
        })
    }

    /// The configuration the loaded build's numbers assume: every option the
    /// build sets that PoB considers relevant to it, plus the enemy type.
    pub fn assumptions(&self) -> Result<Vec<Assumption>> {
        let rows: Vec<Table> = self
            .lua
            .globals()
            .get::<mlua::Function>("bd_assumptions")?
            .call(())?;
        rows.into_iter()
            .map(|row| {
                let kind = match row.get::<String>("kind")?.as_str() {
                    "scenario" => AssumptionKind::Scenario,
                    _ => AssumptionKind::Condition,
                };
                Ok(Assumption {
                    var: row.get("var")?,
                    kind,
                    section: row.get("section")?,
                    label: row.get("label")?,
                    value: row.get("value")?,
                })
            })
            .collect()
    }

    /// Mod lines on equipped items and allocated jewels that PoB ignores.
    pub fn unsupported_mods(&self) -> Result<Vec<UnsupportedMod>> {
        let rows: Vec<Table> = self
            .lua
            .globals()
            .get::<mlua::Function>("bd_unsupported")?
            .call(())?;
        rows.into_iter()
            .map(|row| {
                Ok(UnsupportedMod {
                    slot: row.get("slot")?,
                    item: row.get("item")?,
                    line: row.get("line")?,
                })
            })
            .collect()
    }

    /// Lines PoB printed since the last call.
    pub fn take_log(&self) -> Vec<String> {
        self.lua
            .globals()
            .get::<mlua::Function>("bd_take_log")
            .and_then(|f| f.call::<Vec<String>>(()))
            .unwrap_or_default()
    }

    /// A Lua error without its traceback, plus (when `with_log`) the tail of
    /// PoB's own output, which names the cause when PoB fails to start.
    pub(crate) fn describe(&self, err: &mlua::Error, with_log: bool) -> String {
        let message = match err {
            mlua::Error::RuntimeError(msg) => msg.clone(),
            mlua::Error::CallbackError { cause, .. } => cause.to_string(),
            other => other.to_string(),
        };
        let message = message
            .split("\nstack traceback:")
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned();
        let log = self.take_log();
        if !with_log || log.is_empty() {
            return message;
        }
        let tail: Vec<&str> = log.iter().rev().take(5).rev().map(String::as_str).collect();
        format!("{message}\nPoB output:\n  {}", tail.join("\n  "))
    }
}

/// Up to five lines that differ between two saves, as `before -> after`.
fn read_outputs(values: Table) -> Result<Outputs> {
    values
        .pairs::<String, f64>()
        .map(|pair| pair.map_err(Error::from))
        .collect()
}

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
    fn changed_lines_shows_only_differences() {
        let before = "<a>\n  <Stat value=\"1\"/>\nsame\n{rune}+95 to Spirit";
        let after = "<a>\n  <Stat value=\"2\"/>\nsame\n{rune}+181 to Spirit";
        assert_eq!(
            changed_lines(before, after),
            [
                "<Stat value=\"1\"/> -> <Stat value=\"2\"/>",
                "{rune}+95 to Spirit -> {rune}+181 to Spirit"
            ]
        );
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
            local redirected = pcall(io.output, "doctor-sandbox-probe.txt")
            return f == nil and g == nil and exec == nil and pipe == nil and env == nil
                and removed == nil and not redirected and package.loadlib == nil
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
        for name in ["maxroll_gd79010q.xml", "7occvtBuTEOp.xml"] {
            let xml = fixture(name);
            let first = engine().load(&xml).unwrap();
            assert!(!first.stats.is_empty());
            for _ in 0..4 {
                let again = engine().load(&xml).unwrap();
                assert_eq!(first.stats.diff(&again.stats), [], "{name}");
            }
        }
    }

    fn fixture(name: &str) -> String {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/builds");
        std::fs::read_to_string(dir.join(name)).unwrap()
    }

    /// A build from a passive tree PoB does not know (what a 1.0 build is to
    /// a 0.5-era PoB) must fail with PoB's reason, not produce numbers.
    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn rejects_unknown_tree_version() {
        let xml =
            fixture("maxroll_gd79010q.xml").replace("treeVersion=\"0_5\"", "treeVersion=\"9_9\"");
        let err = engine().load(&xml).unwrap_err();
        assert!(
            matches!(&err, Error::Pob(msg) if msg.contains("Unknown Passive Tree Version")),
            "got {err:?}"
        );
    }

    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn reports_config_assumptions() {
        let engine = engine();
        engine.load(&fixture("maxroll_gd79010q.xml")).unwrap();
        let assumptions = engine.assumptions().unwrap();
        let frozen = assumptions
            .iter()
            .find(|a| a.label.contains("Frozen"))
            .expect("Frozen assumption");
        assert_eq!(frozen.value, "yes");
        assert!(assumptions.iter().any(|a| a.label.contains("Boss")));
    }

    /// Unsetting a condition must change what it should, and leave the
    /// loaded build exactly as it was for every later calculation.
    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn config_what_if_changes_numbers_and_restores_state() {
        let engine = engine();
        let original = engine.load(&fixture("maxroll_gd79010q.xml")).unwrap();
        let frozen = engine
            .assumptions()
            .unwrap()
            .into_iter()
            .find(|a| a.label.contains("Frozen"))
            .unwrap();
        assert_eq!(frozen.kind, AssumptionKind::Condition);

        let without = engine.without_config(&[&frozen.var]).unwrap();
        assert_ne!(
            without.stats.player("CombinedDPS"),
            original.stats.player("CombinedDPS")
        );
        // Unsetting nothing is the original calculation, exactly.
        let same = engine.without_config(&[]).unwrap();
        assert_eq!(original.stats.diff(&same.stats), []);
        // And a fresh load agrees with the engine after what-ifs.
        assert_eq!(
            original
                .stats
                .diff(&engine.load(&fixture("maxroll_gd79010q.xml")).unwrap().stats),
            []
        );
    }

    /// What-ifs spread over a pool give exactly what one engine gives
    /// running them in sequence, in input order, and every worker proves it
    /// loaded the same build.
    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn pool_what_ifs_match_sequential() {
        let xml = fixture("maxroll_gd79010q.xml");
        let engine = engine();
        let base = engine.load(&xml).unwrap();
        let vars: Vec<String> = engine
            .assumptions()
            .unwrap()
            .into_iter()
            .filter(|a| a.kind == AssumptionKind::Condition)
            .take(4)
            .map(|a| a.var)
            .collect();
        let run = |engine: &Engine, var: String| engine.without_config(&[&var]).map(|c| c.stats);
        let pool = crate::Pool::new(&engine.install, 2).unwrap();
        let parallel = pool
            .map(&xml, Some(&base.stats), vars.clone(), run)
            .unwrap();
        let sequential: Vec<_> = vars.into_iter().map(|v| run(&engine, v).unwrap()).collect();
        for (p, s) in parallel.iter().zip(&sequential) {
            assert_eq!(p.diff(s), []);
        }
        // A wrong expectation is refused, not silently accepted.
        let other = engine
            .load(&fixture("7occvtBuTEOp.xml"))
            .unwrap_or(base.clone());
        if other.stats != base.stats {
            assert!(
                pool.map(&xml, Some(&other.stats), vec![()], |_, ()| Ok(()))
                    .is_err()
            );
        }
    }

    /// An exact tree trial reports what changed and leaves the build as it
    /// was (checked by ensure_original's canonical comparison).
    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn tree_trial_changes_the_tree_and_restores_it() {
        let engine = engine();
        let base = engine.load(&fixture("maxroll_gd79010q.xml")).unwrap();
        let screen = engine.tree_screen(4, 0, 1).unwrap();
        let add = screen
            .adds
            .iter()
            .find(|o| o.cost == 1)
            .expect("a 1-point addition");
        let remove = screen
            .removes
            .iter()
            .find(|o| o.cost == 1 && !o.nodes.iter().any(|n| add.nodes.contains(n)))
            .expect("a 1-point removal");
        let trial = engine.try_tree(&[remove.node.id], &[add.node.id]).unwrap();
        assert!(trial.added.iter().any(|n| n.id == add.node.id));
        assert!(trial.removed.iter().any(|n| n.id == remove.node.id));
        assert_eq!(trial.points_after, trial.points_before);
        let restored = engine.ensure_original().unwrap();
        let restored = StatSheet::from_build_xml(&restored).unwrap();
        assert_eq!(base.stats.diff(&restored), []);
    }

    /// A Custom Modifiers what-if changes PoB's numbers as typed, the quick
    /// probe agrees with the settled run, and the build is restored
    /// afterwards. A line PoB cannot read is refused, never ignored.
    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn custom_mod_what_if_changes_and_restores() {
        let engine = engine();
        let base = engine.load(&fixture("maxroll_gd79010q.xml")).unwrap();
        let keys = ["ChaosResist", "MissingChaosResist"];
        let before = engine.outputs(&keys).unwrap();
        let missing = before["MissingChaosResist"];
        assert!(missing > 0.0, "{before:?}");

        let line = format!("+{missing}% to Chaos Resistance");
        let (calc, after) = engine.with_mods(&[&line], &keys).unwrap();
        assert_eq!(after["ChaosResist"], before["ChaosResist"] + missing);
        assert_eq!(after["MissingChaosResist"], 0.0);
        assert_ne!(base.stats.diff(&calc.stats), []);
        let probed = engine.probe_mods(&[&line], &keys).unwrap();
        assert_eq!(probed["ChaosResist"], after["ChaosResist"]);

        let restored = engine.ensure_original().unwrap();
        let restored = StatSheet::from_build_xml(&restored).unwrap();
        assert_eq!(base.stats.diff(&restored), []);
        assert_eq!(engine.outputs(&keys).unwrap(), before);

        let err = engine.with_mods(&["Grants a pony"], &keys).unwrap_err();
        assert!(err.to_string().contains("cannot read"), "{err}");
    }

    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn lists_mods_pob_cannot_parse() {
        let engine = engine();
        engine.load(&fixture("maxroll_gd79010q.xml")).unwrap();
        let unsupported = engine.unsupported_mods().unwrap();
        assert!(
            unsupported
                .iter()
                .any(|m| m.slot == "Body Armour" && m.line.contains("Runic Ward")),
            "{unsupported:?}"
        );
        // Each allocated jewel or slot appears once, not once per lookup path.
        let mut keys: Vec<_> = unsupported.iter().map(|m| (&m.slot, &m.line)).collect();
        let total = keys.len();
        keys.dedup();
        assert_eq!(keys.len(), total);
    }

    #[test]
    #[ignore = "needs a PoB2 install in DOCTOR_POB_DIR"]
    fn rejects_xml_pob_cannot_parse() {
        let err = engine().load("<NotPoB/>").unwrap_err();
        assert!(matches!(err, Error::Pob(_)), "got {err:?}");
    }
}
