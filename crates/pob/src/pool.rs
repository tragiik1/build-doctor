//! Spreading what-ifs on one build over several engines.

use std::collections::VecDeque;
use std::sync::Mutex;
use std::thread;

use crate::{Engine, Error, Install, Result};

impl Engine {
    /// Runs `job` for every item and returns the results in input order.
    ///
    /// `self` must have the build loaded; it does part of the work on the
    /// calling thread. `extra_workers` more engines are started on their own
    /// threads (each costs a PoB start-up and roughly 300 MB), load the same
    /// build, and must reproduce `self`'s saved output exactly before they
    /// take any work. Every engine is checked to be back in its original
    /// state when it finishes.
    pub fn map<I, T, F>(
        &self,
        xml: &str,
        extra_workers: usize,
        items: Vec<I>,
        job: F,
    ) -> Result<Vec<T>>
    where
        I: Send,
        T: Send,
        F: Fn(&Engine, I) -> Result<T> + Sync,
    {
        let expected = self.ensure_original()?;
        let count = items.len();
        let queue = Mutex::new(items.into_iter().enumerate().collect::<VecDeque<_>>());
        let results: Mutex<Vec<Option<T>>> = Mutex::new((0..count).map(|_| None).collect());
        let failure: Mutex<Option<Error>> = Mutex::new(None);

        let work = |engine: &Engine| -> Result<()> {
            loop {
                if failure.lock().expect("poisoned").is_some() {
                    return Ok(());
                }
                let Some((index, item)) = queue.lock().expect("poisoned").pop_front() else {
                    break;
                };
                let value = job(engine, item)?;
                results.lock().expect("poisoned")[index] = Some(value);
            }
            engine.ensure_original().map(drop)
        };
        let record = |result: Result<()>| {
            if let Err(err) = result {
                failure.lock().expect("poisoned").get_or_insert(err);
            }
        };

        let workers = extra_workers.min(count.saturating_sub(1));
        let install = self.install.clone();
        thread::scope(|scope| {
            for _ in 0..workers {
                scope.spawn(|| {
                    record(
                        worker_engine(&install, xml, &expected).and_then(|engine| work(&engine)),
                    );
                });
            }
            record(work(self));
        });

        if let Some(err) = failure.into_inner().expect("poisoned") {
            return Err(err);
        }
        Ok(results
            .into_inner()
            .expect("poisoned")
            .into_iter()
            .map(|r| r.expect("every queued item was processed"))
            .collect())
    }
}

/// A second engine with the same build loaded, proven to agree with the first.
fn worker_engine(install: &Install, xml: &str, expected: &str) -> Result<Engine> {
    let engine = Engine::start(install)?;
    let calc = engine.load(xml)?;
    let reference = crate::StatSheet::from_build_xml(expected)?;
    let mismatches: Vec<String> = reference
        .diff(&calc.stats)
        .iter()
        .map(ToString::to_string)
        .collect();
    if !mismatches.is_empty() {
        return Err(Error::Pob(format!(
            "a second PoB engine calculated the build differently: {}",
            mismatches.join("; ")
        )));
    }
    Ok(engine)
}
