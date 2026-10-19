use std::collections::{HashMap, HashSet};

/// Attempts per app before it is marked failed.
const MAX_ATTEMPTS: u8 = 2;
/// Progress is estimated (brew reports none): it creeps toward this until the process ends.
const CREEP_CAP: f32 = 90.0;
const CREEP_SECS: f32 = 20.0;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Available,
    Queued,
    /// Estimated progress 0..=100.
    Updating(f32),
    Done,
    Failed,
}

/// Update queue: tracks state and hands out work, up to `concurrency` at once.
/// The caller runs the updates and reports back with `finish`.
pub struct Queue {
    status: HashMap<String, Status>,
    batch: Vec<String>,
    attempts: HashMap<String, u8>,
    errors: HashMap<String, String>,
    /// Running when cancelled: their process is being killed and its result must not count.
    cancelled: HashSet<String>,
    concurrency: usize,
}

impl Queue {
    pub fn new(ids: impl IntoIterator<Item = String>, concurrency: usize) -> Self {
        Self {
            status: ids.into_iter().map(|id| (id, Status::Available)).collect(),
            batch: Vec::new(),
            attempts: HashMap::new(),
            errors: HashMap::new(),
            cancelled: HashSet::new(),
            concurrency,
        }
    }

    pub fn status(&self, id: &str) -> Status {
        self.status.get(id).copied().unwrap_or(Status::Available)
    }

    pub fn error(&self, id: &str) -> Option<&str> {
        self.errors.get(id).map(String::as_str)
    }

    pub fn available(&self) -> impl Iterator<Item = &str> {
        self.status.iter().filter(|(_, s)| **s == Status::Available).map(|(id, _)| id.as_str())
    }

    pub fn available_count(&self) -> usize {
        self.available().count()
    }

    pub fn failed_count(&self) -> usize {
        self.status.values().filter(|s| **s == Status::Failed).count()
    }

    pub fn is_busy(&self) -> bool {
        self.status.values().any(|s| matches!(s, Status::Queued | Status::Updating(_)))
    }

    /// Queues the ids that are available (or failed, which makes it a retry).
    /// A new batch starts when idle.
    pub fn enqueue<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) {
        if !self.is_busy() {
            self.batch.clear();
        }
        for id in ids {
            if let Some(s) = self.status.get_mut(id)
                && matches!(*s, Status::Available | Status::Failed)
            {
                *s = Status::Queued;
                self.attempts.remove(id);
                self.errors.remove(id);
                if !self.batch.iter().any(|b| b == id) {
                    self.batch.push(id.to_string());
                }
            }
        }
    }

    /// Drops everything not finished. The caller must also kill the running processes
    /// (`scan::cancel_running`); their late results are ignored.
    pub fn cancel(&mut self) {
        for (id, s) in self.status.iter_mut() {
            match *s {
                Status::Queued => *s = Status::Available,
                Status::Updating(_) => {
                    *s = Status::Available;
                    self.cancelled.insert(id.clone());
                }
                _ => {}
            }
        }
        let status = &self.status;
        self.batch.retain(|id| status.get(id) != Some(&Status::Available));
    }

    /// Moves queued items to updating while there is room; returns the ones just started.
    pub fn start_ready(&mut self) -> Vec<String> {
        let mut running = self.status.values().filter(|s| matches!(s, Status::Updating(_))).count();
        let mut started = Vec::new();
        // Batch order keeps the start order stable (HashMap iteration is not).
        for id in &self.batch {
            if running >= self.concurrency {
                break;
            }
            if let Some(s) = self.status.get_mut(id)
                && *s == Status::Queued
            {
                *s = Status::Updating(0.0);
                running += 1;
                started.push(id.clone());
            }
        }
        started
    }

    /// Advances the estimated progress of running items by `dt` seconds.
    pub fn creep(&mut self, dt: f32) {
        for s in self.status.values_mut() {
            if let Status::Updating(p) = s {
                *p += (CREEP_CAP - *p) * (dt / CREEP_SECS).min(1.0);
            }
        }
    }

    /// The next failure of `id` is final: no silent second attempt (it would ask for the
    /// administrator password again after the user just refused it).
    pub fn no_retry(&mut self, id: &str) {
        self.attempts.insert(id.to_string(), MAX_ATTEMPTS - 1);
    }

    /// Records the outcome of a started update. A failure is retried until attempts run out.
    pub fn finish(&mut self, id: &str, result: Result<(), String>) {
        let Some(s) = self.status.get_mut(id) else { return };
        if self.cancelled.remove(id) {
            // Killed before it ended: back to Available. If it still managed to finish, say so.
            if result.is_ok() {
                *s = Status::Done;
            }
            return;
        }
        match result {
            Ok(()) => *s = Status::Done,
            Err(e) => {
                let n = self.attempts.entry(id.to_string()).or_insert(0);
                *n += 1;
                if *n < MAX_ATTEMPTS {
                    *s = Status::Queued;
                } else {
                    *s = Status::Failed;
                    self.errors.insert(id.to_string(), e);
                }
            }
        }
    }

    /// (done, total, percent) of the current batch.
    pub fn batch_progress(&self) -> (usize, usize, f32) {
        let total = self.batch.len();
        let (mut done, mut sum) = (0, 0.0);
        for id in &self.batch {
            match self.status(id) {
                Status::Done => {
                    done += 1;
                    sum += 100.0;
                }
                // Finished without success: counts toward the bar so it can reach 100%.
                Status::Failed => sum += 100.0,
                Status::Updating(p) => sum += p,
                _ => {}
            }
        }
        (done, total, if total == 0 { 0.0 } else { sum / total as f32 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn q() -> Queue {
        Queue::new(["a", "b", "c"].map(String::from), 2)
    }

    #[test]
    fn respects_concurrency_and_finishes() {
        let mut q = q();
        q.enqueue(["a", "b", "c"]);
        assert_eq!(q.start_ready(), ["a", "b"]);
        assert_eq!(q.status("c"), Status::Queued);
        assert!(q.start_ready().is_empty());
        q.finish("a", Ok(()));
        assert_eq!(q.start_ready(), ["c"]);
        q.finish("b", Ok(()));
        q.finish("c", Ok(()));
        assert!(!q.is_busy());
        assert_eq!(q.batch_progress(), (3, 3, 100.0));
    }

    #[test]
    fn a_refused_password_is_not_retried() {
        let mut q = q();
        q.enqueue(["a"]);
        q.start_ready();
        q.no_retry("a");
        q.finish("a", Err("cancelled".into()));
        assert_eq!(q.status("a"), Status::Failed);
    }

    #[test]
    fn retries_then_fails_and_can_retry_manually() {
        let mut q = q();
        q.enqueue(["a"]);
        q.start_ready();
        q.finish("a", Err("boom".into()));
        assert_eq!(q.status("a"), Status::Queued); // automatic retry
        assert_eq!(q.start_ready(), ["a"]);
        q.finish("a", Err("boom".into()));
        assert_eq!(q.status("a"), Status::Failed);
        assert_eq!(q.error("a"), Some("boom"));
        q.enqueue(["a"]);
        assert_eq!(q.status("a"), Status::Queued);
        assert_eq!(q.error("a"), None);
    }

    #[test]
    fn cancel_stops_running_and_queued() {
        let mut q = q();
        q.enqueue(["a", "b", "c"]);
        q.start_ready(); // a, b running, c queued
        q.cancel();
        assert!(["a", "b", "c"].iter().all(|id| q.status(id) == Status::Available));
        assert!(!q.is_busy());
        // The killed processes report back: no retry, no failure.
        q.finish("a", Err("Cancelado".into()));
        assert_eq!(q.status("a"), Status::Available);
        assert_eq!(q.failed_count(), 0);
        assert!(q.start_ready().is_empty());
        // One that finished anyway counts as done.
        q.finish("b", Ok(()));
        assert_eq!(q.status("b"), Status::Done);
    }

    #[test]
    fn creep_stays_below_cap() {
        let mut q = q();
        q.enqueue(["a"]);
        q.start_ready();
        for _ in 0..1000 {
            q.creep(1.0);
        }
        let Status::Updating(p) = q.status("a") else { panic!() };
        assert!(p <= CREEP_CAP);
    }

    #[test]
    fn retry_in_active_batch_is_not_double_counted() {
        let mut q = q();
        q.enqueue(["a", "b"]);
        q.start_ready();
        q.finish("a", Err("x".into()));
        q.start_ready();
        q.finish("a", Err("x".into()));
        assert_eq!(q.status("a"), Status::Failed);
        q.enqueue(["a"]); // retry while b is still running
        assert_eq!(q.batch_progress().1, 2);
        assert_eq!(q.failed_count(), 0);
    }

    #[test]
    fn enqueue_ignores_non_available() {
        let mut q = q();
        q.enqueue(["a"]);
        q.enqueue(["a", "zzz"]);
        assert_eq!(q.batch_progress().1, 1);
    }
}
