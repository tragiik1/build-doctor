//! A pool of long-lived PoB engines for parallel what-ifs.
//!
//! Each worker thread owns one engine (engines never cross threads). Work is
//! sent as a build plus a list of items; a worker loads the build only when
//! it changes, and the first load of every build is checked against the
//! stats the caller expects, so all workers provably calculate the same
//! build. After its share of the items, every worker checks it is back on
//! the original build.

use std::collections::VecDeque;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::{Engine, Error, Install, Result, StatSheet};

type Task = Box<dyn FnOnce(&mut Slot) + Send>;

/// A worker's engine and the build it has loaded.
struct Slot {
    engine: Engine,
    loaded: Option<Arc<str>>,
}

impl Slot {
    /// Loads `xml` unless it is already loaded, then checks the loaded
    /// build's stats against `expected` (on every call, not only the first).
    fn ensure_loaded(&mut self, xml: &Arc<str>, expected: Option<&StatSheet>) -> Result<()> {
        let stats = if self.loaded.as_deref() == Some(&**xml) {
            match expected {
                Some(_) => StatSheet::from_build_xml(&self.engine.ensure_original()?)?,
                None => return Ok(()),
            }
        } else {
            self.loaded = None;
            let calc = self.engine.load(xml)?;
            self.loaded = Some(Arc::clone(xml));
            calc.stats
        };
        if let Some(expected) = expected {
            let mismatches: Vec<String> = expected
                .diff(&stats)
                .iter()
                .filter(|d| d.is_mismatch())
                .map(ToString::to_string)
                .collect();
            if !mismatches.is_empty() {
                return Err(Error::Pob(format!(
                    "a PoB worker calculated the build differently than expected: {}",
                    mismatches.join("; ")
                )));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Pool {
    workers: Vec<(mpsc::Sender<Task>, thread::JoinHandle<()>)>,
    install: Install,
}

impl Pool {
    /// Starts `size` engines (at least one), each on its own thread. Every
    /// engine costs a PoB start-up and roughly 300 MB.
    pub fn new(install: &Install, size: usize) -> Result<Self> {
        let (ready_tx, ready_rx) = mpsc::channel::<Result<()>>();
        let mut workers = Vec::new();
        for _ in 0..size.max(1) {
            let (tx, rx) = mpsc::channel::<Task>();
            let install = install.clone();
            let ready = ready_tx.clone();
            let handle = thread::spawn(move || {
                let mut slot = match Engine::start(&install) {
                    Ok(engine) => {
                        let _ = ready.send(Ok(()));
                        Slot {
                            engine,
                            loaded: None,
                        }
                    }
                    Err(err) => {
                        let _ = ready.send(Err(err));
                        return;
                    }
                };
                while let Ok(task) = rx.recv() {
                    task(&mut slot);
                }
            });
            workers.push((tx, handle));
        }
        drop(ready_tx);
        for _ in 0..workers.len() {
            ready_rx
                .recv()
                .map_err(|_| Error::Startup("a PoB worker thread exited".into()))??;
        }
        Ok(Self {
            workers,
            install: install.clone(),
        })
    }

    /// The PoB release the pool's engines run.
    pub fn install(&self) -> &Install {
        &self.install
    }

    pub fn size(&self) -> usize {
        self.workers.len()
    }

    /// Runs `job` on every item with `xml` loaded and returns the results in
    /// input order. When `expected` is given, each worker's load of `xml`
    /// must reproduce those stats exactly before it takes work.
    pub fn map<I, T, F>(
        &self,
        xml: &str,
        expected: Option<&StatSheet>,
        items: Vec<I>,
        job: F,
    ) -> Result<Vec<T>>
    where
        I: Send + 'static,
        T: Send + 'static,
        F: Fn(&Engine, I) -> Result<T> + Send + Sync + 'static,
    {
        struct Shared<I, T> {
            queue: Mutex<VecDeque<(usize, I)>>,
            results: Mutex<Vec<Option<T>>>,
            failure: Mutex<Option<Error>>,
        }
        let count = items.len();
        if count == 0 {
            return Ok(Vec::new());
        }
        let shared = Arc::new(Shared {
            queue: Mutex::new(items.into_iter().enumerate().collect()),
            results: Mutex::new((0..count).map(|_| None).collect()),
            failure: Mutex::new(None),
        });
        let job = Arc::new(job);
        let xml: Arc<str> = Arc::from(xml);
        let expected = expected.cloned().map(Arc::new);
        let (done_tx, done_rx) = mpsc::channel::<()>();

        let active = self.workers.len().min(count);
        for (tx, _) in &self.workers[..active] {
            let shared = Arc::clone(&shared);
            let job = Arc::clone(&job);
            let xml = Arc::clone(&xml);
            let expected = expected.clone();
            let done = done_tx.clone();
            let task: Task = Box::new(move |slot: &mut Slot| {
                let result = (|| -> Result<()> {
                    slot.ensure_loaded(&xml, expected.as_deref())?;
                    loop {
                        if lock(&shared.failure).is_some() {
                            return Ok(());
                        }
                        let Some((index, item)) = lock(&shared.queue).pop_front() else {
                            break;
                        };
                        let value = job(&slot.engine, item)?;
                        lock(&shared.results)[index] = Some(value);
                    }
                    slot.engine.ensure_original().map(drop)
                })();
                if let Err(err) = result {
                    lock(&shared.failure).get_or_insert(err);
                    // The engine's state is unknown after a failure.
                    slot.loaded = None;
                }
                let _ = done.send(());
            });
            tx.send(task)
                .map_err(|_| Error::Startup("a PoB worker thread exited".into()))?;
        }
        drop(done_tx);
        for _ in 0..active {
            done_rx
                .recv()
                .map_err(|_| Error::Startup("a PoB worker thread exited".into()))?;
        }

        if let Some(err) = lock(&shared.failure).take() {
            return Err(err);
        }
        let results = std::mem::take(&mut *lock(&shared.results));
        Ok(results
            .into_iter()
            .map(|r| r.expect("every queued item was processed"))
            .collect())
    }
}

impl Drop for Pool {
    fn drop(&mut self) {
        for (tx, handle) in self.workers.drain(..) {
            drop(tx);
            let _ = handle.join();
        }
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
