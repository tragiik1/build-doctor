//! Build Doctor desktop app: a window over the `analysis` crate.
//!
//! The interface (`ui/`) only presents what these commands return; every
//! number comes from Path of Building through `analysis`. Engines are
//! started once, on the first build, and kept warm for later actions.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use analysis::{Goal, Overview, Plan, PlanOptions, PlanProgress, Realistic};
use pob::{DataDir, Pool};
use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

struct AppState {
    data: DataDir,
    pool: Mutex<Option<Arc<Pool>>>,
    /// The loaded build, as the user gave it.
    build: Mutex<Option<String>>,
    cancel: Arc<AtomicBool>,
}

type CommandResult<T> = Result<T, String>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EngineStatus {
    installed: Option<String>,
    pinned: &'static str,
}

#[tauri::command]
fn engine_status(state: State<'_, AppState>) -> EngineStatus {
    EngineStatus {
        installed: state
            .data
            .install(pob::PINNED_VERSION)
            .ok()
            .map(|i| i.version().to_string()),
        pinned: pob::PINNED_VERSION,
    }
}

#[tauri::command]
async fn install_engine(state: State<'_, AppState>) -> CommandResult<String> {
    let dir = state.data.pob_dir();
    blocking(move || {
        let install = pob::Install::download(&format!("v{}", pob::PINNED_VERSION), &dir)?;
        Ok(install.version().to_string())
    })
    .await
}

#[tauri::command]
async fn load_build(state: State<'_, AppState>, input: String) -> CommandResult<Overview> {
    let pool = pool(&state)?;
    let (xml, overview) = blocking(move || {
        let xml = pob::input::read_build(&input)?;
        let overview = analysis::overview(&pool, &xml)?;
        Ok((xml, overview))
    })
    .await?;
    *state.build.lock().map_err(|e| e.to_string())? = Some(xml);
    Ok(overview)
}

#[tauri::command]
async fn realistic(state: State<'_, AppState>, kept: Vec<String>) -> CommandResult<Realistic> {
    let (pool, xml) = (pool(&state)?, build(&state)?);
    blocking(move || Ok(analysis::realistic(&pool, &xml, &kept)?)).await
}

#[tauri::command]
async fn plan(
    app: AppHandle,
    state: State<'_, AppState>,
    kept: Vec<String>,
    goal: Goal,
    steps: usize,
) -> CommandResult<Plan> {
    let (pool, xml) = (pool(&state)?, build(&state)?);
    let cancel = Arc::clone(&state.cancel);
    cancel.store(false, Ordering::SeqCst);
    let options = PlanOptions {
        kept,
        goal,
        steps: steps.clamp(1, 12),
        ..PlanOptions::default()
    };
    blocking(move || {
        let progress = |p: PlanProgress| {
            let _ = app.emit("plan-progress", p);
        };
        Ok(analysis::plan(&pool, &xml, &options, &progress, &|| {
            cancel.load(Ordering::SeqCst)
        })?)
    })
    .await
}

#[tauri::command]
fn cancel_plan(state: State<'_, AppState>) {
    state.cancel.store(true, Ordering::SeqCst);
}

/// The warm engine pool, started on first use with one engine per core (at
/// most 8; each holds roughly 300 MB).
fn pool(state: &AppState) -> CommandResult<Arc<Pool>> {
    let mut slot = state.pool.lock().map_err(|e| e.to_string())?;
    if let Some(pool) = slot.as_ref() {
        return Ok(Arc::clone(pool));
    }
    let install = state
        .data
        .install(pob::PINNED_VERSION)
        .map_err(|e| e.to_string())?;
    let size = std::thread::available_parallelism()
        .map_or(2, |n| n.get())
        .min(8);
    let pool = Arc::new(Pool::new(&install, size).map_err(|e| e.to_string())?);
    *slot = Some(Arc::clone(&pool));
    Ok(pool)
}

fn build(state: &AppState) -> CommandResult<String> {
    state
        .build
        .lock()
        .map_err(|e| e.to_string())?
        .clone()
        .ok_or_else(|| "No build loaded.".to_owned())
}

/// Runs PoB work off the UI thread.
async fn blocking<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, Box<dyn std::error::Error + Send + Sync>> + Send + 'static,
) -> CommandResult<T> {
    tauri::async_runtime::spawn_blocking(move || work().map_err(|e| e.to_string()))
        .await
        .map_err(|e| e.to_string())?
}

fn main() {
    let data = match DataDir::new(None) {
        Ok(data) => data,
        Err(err) => {
            eprintln!("Build Doctor cannot find a data folder: {err}");
            std::process::exit(1);
        }
    };
    tauri::Builder::default()
        .manage(AppState {
            data,
            pool: Mutex::new(None),
            build: Mutex::new(None),
            cancel: Arc::new(AtomicBool::new(false)),
        })
        .invoke_handler(tauri::generate_handler![
            engine_status,
            install_engine,
            load_build,
            realistic,
            plan,
            cancel_plan
        ])
        .run(tauri::generate_context!())
        .unwrap_or_else(|err| {
            eprintln!("Build Doctor could not start: {err}");
            std::process::exit(1);
        });
}
