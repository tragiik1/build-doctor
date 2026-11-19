//! `doctor`: Path of Exile 2 build doctor.

mod check;
mod conditions;
mod fixtures;
mod realistic;
mod todo;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

use pob::PINNED_VERSION as PINNED_POB;

#[derive(Debug, Parser)]
#[command(name = "doctor", version, about = "Path of Exile 2 build doctor")]
struct Cli {
    /// Where PoB installs and caches live. Defaults to the platform data
    /// directory (%LOCALAPPDATA%\build-doctor on Windows).
    #[arg(long, global = true, env = "DOCTOR_DATA_DIR")]
    data_dir: Option<PathBuf>,

    /// Print check, realistic and todo results as JSON instead of text.
    #[arg(long, global = true)]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Manage the Path of Building engine.
    #[command(subcommand)]
    Engine(EngineCommand),

    /// Calculate a build and compare PoB's fresh numbers with the ones saved
    /// in the build.
    Check {
        /// A PoB code, a path to a code or XML file, or a pobb.in /
        /// poe.ninja / maxroll / poe2db build link.
        build: String,
        /// PoB version to calculate with.
        #[arg(long, default_value = PINNED_POB)]
        pob: String,
        /// List every stat, not just the summary.
        #[arg(long)]
        all: bool,
    },

    /// Show what each combat condition the build assumes is worth, and its
    /// numbers without them (or with only the ones you keep).
    Realistic {
        /// A PoB code, a path to a code or XML file, or a build link.
        build: String,
        /// Keep conditions whose label contains this text (case-insensitive),
        /// for conditions you really sustain. Repeatable.
        #[arg(long)]
        keep: Vec<String>,
        #[arg(long, default_value = PINNED_POB)]
        pob: String,
        /// PoB engines to run in parallel (about 300 MB each). Defaults to
        /// the number of CPU cores, at most 8.
        #[arg(long)]
        jobs: Option<usize>,
    },

    /// The ranked to-do list: changes that improve the build most, each
    /// applied and recalculated exactly by PoB. Covers the passive tree.
    Todo {
        /// A PoB code, a path to a code or XML file, or a build link.
        build: String,
        /// What to improve.
        #[arg(long, value_enum, default_value = "damage")]
        goal: GoalArg,
        /// Combat conditions you really sustain (as in `doctor realistic`);
        /// all others are removed before scoring. Repeatable.
        #[arg(long)]
        keep: Vec<String>,
        /// Consider nodes up to this many points away.
        #[arg(long, default_value_t = 6)]
        max_distance: u32,
        /// At most this many steps.
        #[arg(long, default_value_t = 5)]
        steps: usize,
        /// Write the final tree as a PoB code to this file.
        #[arg(long)]
        export: Option<PathBuf>,
        #[arg(long, default_value = PINNED_POB)]
        pob: String,
        /// PoB engines to run in parallel (about 300 MB each).
        #[arg(long)]
        jobs: Option<usize>,
    },

    /// Run the fixture suite: every fixture build must reproduce its saved
    /// stats exactly on each PoB version it is recorded against.
    Verify {
        /// Fixture directory containing fixtures.toml.
        #[arg(long, default_value = "fixtures")]
        fixtures: PathBuf,
        /// Also run every fixture on every installed PoB version and print
        /// the match matrix (for recording new fixtures).
        #[arg(long)]
        sweep: bool,
    },
}

#[derive(Debug, Subcommand)]
enum EngineCommand {
    /// Download a PoB2 release from GitHub and verify every file.
    Install {
        #[arg(long, default_value = PINNED_POB)]
        version: String,
        /// Install from a release zip already on disk instead of downloading
        /// (the "Source code (zip)" of a PoB2 GitHub release).
        #[arg(long, conflicts_with = "version")]
        archive: Option<PathBuf>,
    },
    /// List installed PoB2 versions.
    List,
    /// Re-hash an installed version against PoB's manifest.
    Verify {
        #[arg(long, default_value = PINNED_POB)]
        version: String,
    },
}

#[derive(Debug, Clone, Copy, clap::ValueEnum)]
enum GoalArg {
    /// More damage; survival changes are shown alongside.
    Damage,
    /// A larger smallest max hit.
    Survival,
}

/// The installed PoB release `version`, with the command that installs it
/// when it is missing.
fn installed(data: &pob::DataDir, version: &str) -> AnyResult<pob::Install> {
    data.install(version).map_err(|err| match err {
        pob::Error::NotInstalled(v) => {
            format!("PoB2 {v} is not installed. Run: doctor engine install --version {v}").into()
        }
        other => other.into(),
    })
}

fn print_json(value: &impl serde::Serialize) -> AnyResult<ExitCode> {
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(ExitCode::SUCCESS)
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map_or(1, |n| n.get())
        .min(8)
}

fn main() -> ExitCode {
    // Exit quietly when output is piped into something that stops reading
    // (e.g. `| head`), as command-line tools do, instead of panicking.
    #[cfg(unix)]
    // SAFETY: restoring the default SIGPIPE disposition before any threads exist.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("error: {err}");
            ExitCode::FAILURE
        }
    }
}

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn run(cli: Cli) -> AnyResult<ExitCode> {
    let data = pob::DataDir::new(cli.data_dir)?;
    match cli.command {
        Command::Engine(EngineCommand::Install { version, archive }) => {
            let install = match archive {
                Some(zip) => pob::Install::from_archive(&zip, &data.pob_dir())?,
                None => {
                    let tag = format!("v{}", version.trim_start_matches('v'));
                    println!("Downloading PoB2 {tag} into {}", data.pob_dir().display());
                    pob::Install::download(&tag, &data.pob_dir())?
                }
            };
            println!(
                "Installed and verified PoB2 {} at {}",
                install.version(),
                install.root().display()
            );
        }
        Command::Engine(EngineCommand::List) => {
            let installs = data.installs()?;
            if installs.is_empty() {
                println!("No PoB2 versions installed. Run: doctor engine install");
            }
            for install in installs {
                let pinned = if install.version().as_str() == PINNED_POB {
                    "  (pinned)"
                } else {
                    ""
                };
                println!(
                    "{}{pinned}  {}",
                    install.version(),
                    install.root().display()
                );
            }
        }
        Command::Engine(EngineCommand::Verify { version }) => {
            let install = installed(&data, &version)?;
            install.verify()?;
            println!("PoB2 {} matches its manifest.", install.version());
        }
        Command::Check { build, pob, all } => {
            let pool = pob::Pool::new(&installed(&data, &pob)?, 1)?;
            let xml = pob::input::read_build(&build)?;
            if cli.json {
                return print_json(&analysis::overview(&pool, &xml)?);
            }
            return check::run(&pool, &xml, all);
        }
        Command::Realistic {
            build,
            keep,
            pob,
            jobs,
        } => {
            let pool = pob::Pool::new(&installed(&data, &pob)?, jobs.unwrap_or_else(default_jobs))?;
            let xml = pob::input::read_build(&build)?;
            let kept = conditions::kept_vars(&pool, &xml, &keep)?;
            if cli.json {
                return print_json(&analysis::realistic(&pool, &xml, &kept)?);
            }
            return realistic::run(&pool, &xml, &kept);
        }
        Command::Todo {
            build,
            goal,
            keep,
            max_distance,
            steps,
            export,
            pob,
            jobs,
        } => {
            let pool = pob::Pool::new(&installed(&data, &pob)?, jobs.unwrap_or_else(default_jobs))?;
            let xml = pob::input::read_build(&build)?;
            let options = analysis::PlanOptions {
                kept: conditions::kept_vars(&pool, &xml, &keep)?,
                goal: match goal {
                    GoalArg::Damage => analysis::Goal::Damage,
                    GoalArg::Survival => analysis::Goal::Survival,
                },
                max_distance,
                steps,
            };
            if cli.json {
                return print_json(&analysis::plan(&pool, &xml, &options, &|_| {}, &|| false)?);
            }
            return todo::run(&pool, &xml, &options, export);
        }
        Command::Verify { fixtures, sweep } => {
            return fixtures::verify(&data, &fixtures, sweep);
        }
    }
    Ok(ExitCode::SUCCESS)
}
