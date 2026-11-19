//! `doctor verify`: the precision gate.
//!
//! Each fixture is a real build saved by the PoB2 desktop app, so its saved
//! stats are the app's own output. On every PoB version listed in `exact_on`,
//! the doctor must reproduce every saved stat string-for-string. Anything
//! less is a bug.

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use pob::{Engine, Install, StatSheet};
use serde::Deserialize;

use crate::AnyResult;
use pob::{DataDir, version_key};

#[derive(Debug, Deserialize)]
struct Manifest {
    fixture: Vec<Fixture>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Fixture {
    /// Path relative to the fixture directory.
    file: PathBuf,
    /// Where the build came from.
    source: String,
    /// Short human description.
    build: String,
    /// PoB versions that must reproduce every saved stat.
    #[serde(default)]
    exact_on: Vec<String>,
    /// Why a fixture is not used as an oracle, if it is not.
    quarantine: Option<String>,
}

impl Fixture {
    fn id(&self) -> &str {
        self.file
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
    }
}

struct Loaded<'a> {
    fixture: &'a Fixture,
    xml: String,
    saved: StatSheet,
}

pub fn verify(data: &DataDir, dir: &Path, sweep: bool) -> AnyResult<ExitCode> {
    let manifest_path = dir.join("fixtures.toml");
    let manifest: Manifest = toml::from_str(&fs::read_to_string(&manifest_path)?)
        .map_err(|e| format!("{}: {e}", manifest_path.display()))?;

    let mut loaded = Vec::new();
    for fixture in &manifest.fixture {
        let path = dir.join(&fixture.file);
        let xml = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let saved = StatSheet::from_build_xml(&xml)?;
        if saved.is_empty() {
            return Err(format!("{}: no saved stats, cannot be a fixture", path.display()).into());
        }
        loaded.push(Loaded {
            fixture,
            xml,
            saved,
        });
    }

    let installs = data.installs()?;
    if sweep {
        return run_sweep(&installs, &loaded);
    }
    run_gate(&installs, &loaded)
}

fn run_gate(installs: &[Install], fixtures: &[Loaded<'_>]) -> AnyResult<ExitCode> {
    let wanted: BTreeSet<&str> = fixtures
        .iter()
        .filter(|f| f.fixture.quarantine.is_none())
        .flat_map(|f| f.fixture.exact_on.iter().map(String::as_str))
        .collect();

    let (mut exact, mut failed, mut skipped) = (0usize, 0usize, Vec::new());
    for version in wanted {
        let Some(install) = installs.iter().find(|i| i.version().as_str() == version) else {
            let count = fixtures
                .iter()
                .filter(|f| {
                    f.fixture.quarantine.is_none()
                        && f.fixture.exact_on.iter().any(|v| v == version)
                })
                .count();
            skipped.push(format!("{count} on PoB2 {version} (not installed)"));
            continue;
        };
        println!("PoB2 {version}");
        let engine = Engine::start(install)?;
        let cases: Vec<_> = fixtures
            .iter()
            .filter(|f| {
                f.fixture.quarantine.is_none() && f.fixture.exact_on.iter().any(|v| v == version)
            })
            .collect();

        let mut first_result = None;
        for case in &cases {
            let started = Instant::now();
            let outcome = engine.load(&case.xml);
            let elapsed = started.elapsed().as_secs_f64();
            match outcome {
                Ok(calc) => {
                    let mismatches: Vec<_> = case
                        .saved
                        .diff(&calc.stats)
                        .into_iter()
                        .filter(|d| d.is_mismatch())
                        .collect();
                    if mismatches.is_empty() {
                        exact += 1;
                        println!(
                            "  exact  {:<17} {:>4} stats  {elapsed:>5.2}s  {}",
                            case.fixture.id(),
                            case.saved.len(),
                            case.fixture.build
                        );
                    } else {
                        failed += 1;
                        println!(
                            "  FAIL   {:<17} {}/{} stats differ ({})",
                            case.fixture.id(),
                            mismatches.len(),
                            case.saved.len(),
                            case.fixture.source
                        );
                        for diff in mismatches {
                            println!("           {diff}");
                        }
                    }
                    if first_result.is_none() {
                        first_result = Some(calc.stats);
                    }
                }
                Err(err) => {
                    failed += 1;
                    println!("  FAIL   {:<17} {err}", case.fixture.id());
                }
            }
        }

        // Reusing one engine for many builds must not leak state between them.
        if let (Some(first), Some(expected)) = (cases.first(), first_result)
            && cases.len() > 1
        {
            let again = engine.load(&first.xml)?;
            if expected.diff(&again.stats).is_empty() {
                println!(
                    "  clean  engine reuse: reloading {} after the others gives identical output",
                    first.fixture.id()
                );
            } else {
                failed += 1;
                println!(
                    "  FAIL   engine reuse: reloading {} gave different output",
                    first.fixture.id()
                );
            }
        }
    }

    let quarantined: Vec<_> = fixtures
        .iter()
        .filter(|f| f.fixture.quarantine.is_some())
        .collect();
    if !quarantined.is_empty() {
        println!("Quarantined (not used as oracles)");
        for f in quarantined {
            println!(
                "  {:<17} {}",
                f.fixture.id(),
                f.fixture.quarantine.as_deref().unwrap_or_default()
            );
        }
    }

    println!();
    print!("{exact} exact, {failed} failed");
    if !skipped.is_empty() {
        print!(", skipped {}", skipped.join(", "));
    }
    println!();
    Ok(if failed == 0 {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn run_sweep(installs: &[Install], fixtures: &[Loaded<'_>]) -> AnyResult<ExitCode> {
    if installs.is_empty() {
        return Err("no PoB versions installed".into());
    }
    let mut versions: Vec<&Install> = installs.iter().collect();
    versions.sort_by_key(|i| version_key(i.version().as_str()));

    print!("{:<17}", "fixture");
    for install in &versions {
        print!(" {:>8}", install.version().as_str());
    }
    println!();

    let mut grid = vec![vec![String::new(); versions.len()]; fixtures.len()];
    for (col, install) in versions.iter().enumerate() {
        let engine = Engine::start(install)?;
        for (row, case) in fixtures.iter().enumerate() {
            grid[row][col] = match engine.load(&case.xml) {
                Ok(calc) => match case
                    .saved
                    .diff(&calc.stats)
                    .iter()
                    .filter(|d| d.is_mismatch())
                    .count()
                {
                    0 => "exact".into(),
                    n => n.to_string(),
                },
                Err(pob::Error::Unsettled { .. }) => "unstable".into(),
                Err(_) => "error".into(),
            };
        }
    }
    for (row, case) in fixtures.iter().enumerate() {
        print!("{:<17}", case.fixture.id());
        for cell in &grid[row] {
            print!(" {cell:>8}");
        }
        println!();
    }
    println!("\nCells: `exact`; the number of saved stats that differ; `unstable` when PoB's");
    println!(
        "output never settles (a PoB bug, see `doctor check`); `error` when PoB fails to load it."
    );
    Ok(ExitCode::SUCCESS)
}
