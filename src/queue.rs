use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Status {
    Available,
    Queued,
    /// Progress 0..=100.
    Updating(f32),
    Done,
}

/// Update queue: runs up to `concurrency` updates at once. It only tracks state;
/// whoever drives it reports progress through `tick`.
pub struct Queue {
    status: HashMap<String, Status>,
    batch: Vec<String>,
    concurrency: usize,
}

impl Queue {
    pub fn new(ids: impl IntoIterator<Item = String>, concurrency: usize) -> Self {
        Self {
            status: ids.into_iter().map(|id| (id, Status::Available)).collect(),
            batch: Vec::new(),
            concurrency,
        }
    }

    pub fn status(&self, id: &str) -> Status {
        self.status.get(id).copied().unwrap_or(Status::Available)
    }

    pub fn available(&self) -> impl Iterator<Item = &str> {
        self.status.iter().filter(|(_, s)| **s == Status::Available).map(|(id, _)| id.as_str())
    }

    pub fn available_count(&self) -> usize {
        self.available().count()
    }

    pub fn is_busy(&self) -> bool {
        self.status.values().any(|s| matches!(s, Status::Queued | Status::Updating(_)))
    }

    /// Queues the ids that are still available; a new batch starts when idle.
    pub fn enqueue<'a>(&mut self, ids: impl IntoIterator<Item = &'a str>) {
        if !self.is_busy() {
            self.batch.clear();
        }
        for id in ids {
            if let Some(s) = self.status.get_mut(id)
                && *s == Status::Available
            {
                *s = Status::Queued;
                self.batch.push(id.to_string());
            }
        }
    }

    /// Back to available for everything queued or in flight.
    pub fn cancel(&mut self) {
        for s in self.status.values_mut() {
            if matches!(s, Status::Queued | Status::Updating(_)) {
                *s = Status::Available;
            }
        }
        self.batch.clear();
    }

    /// Starts queued items up to the concurrency limit, then adds `advance(id)`
    /// percentage points to each running one.
    pub fn tick(&mut self, advance: impl Fn(&str) -> f32) {
        let mut running = self.status.values().filter(|s| matches!(s, Status::Updating(_))).count();
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
            }
        }
        for (id, s) in self.status.iter_mut() {
            if let Status::Updating(p) = *s {
                let p = p + advance(id);
                *s = if p >= 100.0 { Status::Done } else { Status::Updating(p) };
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
        q.tick(|_| 40.0);
        assert_eq!(q.status("a"), Status::Updating(40.0));
        assert_eq!(q.status("b"), Status::Updating(40.0));
        assert_eq!(q.status("c"), Status::Queued);
        q.tick(|_| 60.0);
        assert_eq!(q.status("a"), Status::Done);
        q.tick(|_| 60.0);
        assert_eq!(q.status("c"), Status::Updating(60.0));
        q.tick(|_| 60.0);
        assert!(!q.is_busy());
        assert_eq!(q.batch_progress(), (3, 3, 100.0));
    }

    #[test]
    fn cancel_restores_available_and_skips_done() {
        let mut q = q();
        q.enqueue(["a", "b", "c"]);
        q.tick(|_| 100.0); // a, b done
        q.cancel();
        assert_eq!(q.status("a"), Status::Done);
        assert_eq!(q.status("c"), Status::Available);
        assert_eq!(q.available_count(), 1);
    }

    #[test]
    fn enqueue_ignores_non_available() {
        let mut q = q();
        q.enqueue(["a"]);
        q.enqueue(["a", "zzz"]);
        assert_eq!(q.batch_progress().1, 1);
    }
}
