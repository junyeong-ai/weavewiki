use std::collections::HashMap;

const MAX_TRACKED_PAIRS: usize = 5_000;
const PRUNE_BATCH_SIZE: usize = 500;

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
struct FailureKey {
    artifact: String,
    strategy: String,
}

impl FailureKey {
    fn new(artifact: impl Into<String>, strategy: impl Into<String>) -> Self {
        Self {
            artifact: artifact.into(),
            strategy: strategy.into(),
        }
    }
}

#[derive(Debug, Clone)]
struct FailureRecord {
    count: usize,
    last_recorded: u64,
}

pub struct FailureTracker {
    failures: HashMap<FailureKey, FailureRecord>,
    max_failures: usize,
    tick: u64,
}

impl Default for FailureTracker {
    fn default() -> Self {
        Self::new(2)
    }
}

impl FailureTracker {
    pub fn new(max_failures: usize) -> Self {
        Self {
            failures: HashMap::new(),
            max_failures,
            tick: 0,
        }
    }

    pub fn should_skip(&self, artifact: &str, strategy: &str) -> bool {
        let key = FailureKey::new(artifact, strategy);
        self.failures
            .get(&key)
            .map(|r| r.count >= self.max_failures)
            .unwrap_or(false)
    }

    pub fn record_failure(&mut self, artifact: &str, strategy: &str) {
        self.tick += 1;

        if self.failures.len() >= MAX_TRACKED_PAIRS {
            self.prune_oldest();
        }

        let key = FailureKey::new(artifact, strategy);
        self.failures
            .entry(key)
            .and_modify(|r| {
                r.count += 1;
                r.last_recorded = self.tick;
            })
            .or_insert(FailureRecord {
                count: 1,
                last_recorded: self.tick,
            });
    }

    pub fn record_success(&mut self, artifact: &str, strategy: &str) {
        let key = FailureKey::new(artifact, strategy);
        self.failures.remove(&key);
    }

    pub fn failure_count(&self, artifact: &str, strategy: &str) -> usize {
        let key = FailureKey::new(artifact, strategy);
        self.failures.get(&key).map(|r| r.count).unwrap_or(0)
    }

    fn prune_oldest(&mut self) {
        if self.failures.len() < PRUNE_BATCH_SIZE {
            return;
        }

        let mut entries: Vec<_> = self
            .failures
            .iter()
            .map(|(k, v)| (k.clone(), v.last_recorded))
            .collect();

        entries.sort_by_key(|(_, tick)| *tick);

        for (key, _) in entries.into_iter().take(PRUNE_BATCH_SIZE) {
            self.failures.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skip_after_max_failures() {
        let mut tracker = FailureTracker::new(2);

        assert!(!tracker.should_skip("skill-a", "evidence"));

        tracker.record_failure("skill-a", "evidence");
        assert!(!tracker.should_skip("skill-a", "evidence"));

        tracker.record_failure("skill-a", "evidence");
        assert!(tracker.should_skip("skill-a", "evidence"));
    }

    #[test]
    fn test_success_resets() {
        let mut tracker = FailureTracker::new(2);

        tracker.record_failure("skill-a", "evidence");
        tracker.record_failure("skill-a", "evidence");
        assert!(tracker.should_skip("skill-a", "evidence"));

        tracker.record_success("skill-a", "evidence");
        assert!(!tracker.should_skip("skill-a", "evidence"));
    }

    #[test]
    fn test_different_artifacts_independent() {
        let mut tracker = FailureTracker::new(2);

        tracker.record_failure("skill-a", "evidence");
        tracker.record_failure("skill-a", "evidence");

        assert!(tracker.should_skip("skill-a", "evidence"));
        assert!(!tracker.should_skip("skill-b", "evidence"));
    }

    #[test]
    fn test_prune_respects_recency() {
        let mut tracker = FailureTracker::new(3);

        for i in 0..100 {
            tracker.record_failure(&format!("artifact-{}", i), "strategy");
        }

        tracker.record_failure("artifact-50", "strategy");

        assert_eq!(tracker.failure_count("artifact-50", "strategy"), 2);
    }

    #[test]
    fn test_failure_key_equality() {
        let key1 = FailureKey::new("artifact", "strategy");
        let key2 = FailureKey::new("artifact", "strategy");
        let key3 = FailureKey::new("artifact", "other");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }
}
