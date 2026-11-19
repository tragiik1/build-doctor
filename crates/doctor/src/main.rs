//! `doctor`: Path of Exile 2 build doctor.

mod check;
mod data_dir;
mod fixtures;
mod input;
mod realistic;

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

/// PoB2 release the doctor calculates with. Matches the current PoB2 app
/// release; bump it, then run `doctor verify`, when PoB2 ships an update.
pub const PINNED_POB: &str = "0.23.1";

#[derive(Debug, Parser)]
#[command(name = "doctor", version, about = "Path of Exile 2 build doctor")]
struct Cli {
    /// Where PoB installs and caches live. Defaults to the platform data
    /// directory (%LOCALAPPDATA%\build-doctor on Windows).
    #[arg(long, global = true, env = "DOCTOR_DATA_DIR")]
    data_dir: Option<PathBuf>,

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
    let data = data_dir::DataDir::new(cli.data_dir)?;
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
            let install = data.install(&version)?;
            install.verify()?;
            println!("PoB2 {} matches its manifest.", install.version());
        }
        Command::Check { build, pob, all } => {
            let install = data.install(&pob)?;
            let xml = input::read_build(&build)?;
            return check::run(&install, &xml, all);
        }
        Command::Realistic {
            build,
            keep,
            pob,
            jobs,
        } => {
            let install = data.install(&pob)?;
            let xml = input::read_build(&build)?;
            let jobs = jobs.unwrap_or_else(default_jobs).max(1);
            return realistic::run(&install, &xml, &keep, jobs);
        }
        Command::Verify { fixtures, sweep } => {
            return fixtures::verify(&data, &fixtures, sweep);
        }
    }
    Ok(ExitCode::SUCCESS)
}
