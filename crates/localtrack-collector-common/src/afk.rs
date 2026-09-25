use localtrack_core::activity::{IdleObservation, Observation};

/// Turns "time of last user input" into idle/active observations (spec §26).
///
/// The AFK period logically begins at `last_input + threshold`, not when the
/// polling loop happened to notice it.
#[derive(Debug, Clone)]
pub struct AfkTracker {
    threshold_ms: i64,
    is_idle: bool,
    last_input_ms: i64,
}

impl AfkTracker {
    pub fn new(threshold_ms: i64) -> Self {
        Self {
            threshold_ms,
            is_idle: false,
            last_input_ms: 0,
        }
    }

    pub fn set_threshold_ms(&mut self, threshold_ms: i64) {
        self.threshold_ms = threshold_ms;
    }

    pub fn threshold_ms(&self) -> i64 {
        self.threshold_ms
    }

    pub fn is_idle(&self) -> bool {
        self.is_idle
    }

    /// Feed the current idle time; returns an observation on state change.
    pub fn update(&mut self, idle_for_ms: i64, now_ms: i64) -> Option<Observation> {
        let last_input_ms = now_ms - idle_for_ms.max(0);
        self.last_input_ms = last_input_ms;

        let observation = IdleObservation {
            captured_at_ms: now_ms,
            last_input_ms,
            idle_threshold_ms: self.threshold_ms,
        };

        if idle_for_ms >= self.threshold_ms {
            if !self.is_idle {
                self.is_idle = true;
                return Some(Observation::UserIdle(observation));
            }
            None
        } else {
            if self.is_idle {
                self.is_idle = false;
                return Some(Observation::UserActive(observation));
            }
            None
        }
    }

    /// A system lock forces idle immediately, overriding the threshold
    /// (spec §28).
    pub fn force_idle(&mut self) {
        self.is_idle = true;
    }

    pub fn force_active(&mut self, now_ms: i64) {
        self.is_idle = false;
        self.last_input_ms = now_ms;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_segment_starts_at_last_input_plus_threshold() {
        let mut tracker = AfkTracker::new(180_000);
        assert!(tracker.update(1_000, 601_000).is_none());
        // Noticed at 604_000 after 184s of idleness.
        let observation = tracker.update(184_000, 604_000).expect("idle observation");
        match observation {
            Observation::UserIdle(idle) => {
                assert_eq!(idle.last_input_ms, 420_000);
                assert_eq!(idle.afk_started_at_ms(), 600_000);
            }
            other => panic!("unexpected observation {other:?}"),
        }
    }

    #[test]
    fn only_state_changes_produce_observations() {
        let mut tracker = AfkTracker::new(1_000);
        assert!(tracker.update(5_000, 10_000).is_some());
        assert!(tracker.update(6_000, 11_000).is_none());
        assert!(tracker.update(0, 12_000).is_some());
        assert!(tracker.update(0, 13_000).is_none());
    }

    #[test]
    fn lock_forces_idle_without_waiting_for_the_threshold() {
        let mut tracker = AfkTracker::new(180_000);
        tracker.force_idle();
        assert!(tracker.is_idle());
        tracker.force_active(1_000);
        assert!(!tracker.is_idle());
    }
}
