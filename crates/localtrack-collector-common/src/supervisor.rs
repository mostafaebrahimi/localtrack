/// Bounded exponential backoff used when a collector or connection fails
/// (spec §37, §145).
#[derive(Debug, Clone)]
pub struct Backoff {
    steps_ms: Vec<u64>,
    attempt: usize,
}

impl Default for Backoff {
    fn default() -> Self {
        // Spec §37: 1s, 2s, 5s, 10s, 30s, capped at 30s.
        Self {
            steps_ms: vec![1_000, 2_000, 5_000, 10_000, 30_000],
            attempt: 0,
        }
    }
}

impl Backoff {
    pub fn new(steps_ms: Vec<u64>) -> Self {
        Self {
            steps_ms,
            attempt: 0,
        }
    }

    /// Next delay in milliseconds; never grows past the last step.
    pub fn next_delay_ms(&mut self) -> u64 {
        let index = self.attempt.min(self.steps_ms.len() - 1);
        self.attempt += 1;
        self.steps_ms[index]
    }

    pub fn reset(&mut self) {
        self.attempt = 0;
    }

    pub fn attempts(&self) -> usize {
        self.attempt
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn follows_the_documented_schedule_and_caps() {
        let mut backoff = Backoff::default();
        assert_eq!(backoff.next_delay_ms(), 1_000);
        assert_eq!(backoff.next_delay_ms(), 2_000);
        assert_eq!(backoff.next_delay_ms(), 5_000);
        assert_eq!(backoff.next_delay_ms(), 10_000);
        assert_eq!(backoff.next_delay_ms(), 30_000);
        assert_eq!(backoff.next_delay_ms(), 30_000, "never exceeds 30 seconds");
        backoff.reset();
        assert_eq!(backoff.next_delay_ms(), 1_000);
    }
}
