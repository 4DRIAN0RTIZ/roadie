//! Background loading of repo summaries: a small worker pool over `mpsc`.
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::backend::Backend;
use crate::domain::RepoSummary;

pub const WORKERS: usize = 5;

type Outcome = (String, Result<RepoSummary, String>);

pub struct SummaryLoader {
    jobs: Sender<String>,
    results: Receiver<Outcome>,
}

impl SummaryLoader {
    /// Spawns `workers` detached threads; they end when the loader is dropped
    /// (or with the process, if blocked inside a `gh` call).
    pub fn spawn(be: Arc<dyn Backend>, workers: usize) -> Self {
        let (jobs, job_rx) = channel::<String>();
        let (res_tx, results) = channel::<Outcome>();
        let job_rx = Arc::new(Mutex::new(job_rx));
        for _ in 0..workers.max(1) {
            let (be, job_rx, res_tx) = (be.clone(), job_rx.clone(), res_tx.clone());
            thread::spawn(move || loop {
                let job = match job_rx.lock() {
                    Ok(rx) => rx.recv(),
                    Err(_) => return,
                };
                let Ok(repo) = job else { return };
                let out = be.repo_summary(&repo).map_err(|e| format!("{e:#}"));
                if res_tx.send((repo, out)).is_err() {
                    return;
                }
            });
        }
        Self { jobs, results }
    }

    pub fn submit(&self, repo: String) {
        let _ = self.jobs.send(repo);
    }

    /// Everything that finished since the last call; never blocks.
    pub fn poll(&self) -> Vec<Outcome> {
        self.results.try_iter().collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::fake::FakeBackend;
    use std::time::{Duration, Instant};

    #[test]
    fn delivers_results_and_errors_per_repo() {
        let mut be = FakeBackend::default();
        be.summaries.insert(
            "a".into(),
            RepoSummary {
                counts: [1, 0, 0, 0],
                ..Default::default()
            },
        );
        let loader = SummaryLoader::spawn(Arc::new(be), 2);
        loader.submit("a".into());
        loader.submit("missing".into());
        let mut got = vec![];
        let end = Instant::now() + Duration::from_secs(3);
        while got.len() < 2 && Instant::now() < end {
            got.extend(loader.poll());
            thread::sleep(Duration::from_millis(5));
        }
        got.sort_by(|x, y| x.0.cmp(&y.0));
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "a");
        assert_eq!(got[0].1.as_ref().unwrap().counts[0], 1);
        assert!(got[1].1.is_err());
    }
}
