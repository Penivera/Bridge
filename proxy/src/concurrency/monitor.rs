use std::collections::VecDeque;
use std::time::Duration;

/// Default capacity for the sliding request dwell buffer window (20 requests).
pub const SLIDING_WINDOW_SIZE: usize = 20;

/// Default queue dwell latency threshold triggering scale-up (5ms).
pub const DEFAULT_SCALE_UP_THRESHOLD: Duration = Duration::from_millis(5);

/// Sliding window monitor that tracks queuing delay (dwell time) across the last 20 requests.
#[derive(Debug, Clone)]
pub struct QueueDwellMonitor {
    samples: VecDeque<Duration>,
    capacity: usize,
}

impl QueueDwellMonitor {
    pub fn new(capacity: usize) -> Self {
        Self {
            samples: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    /// Records an observed connection queue dwell duration into the rolling window.
    pub fn record(&mut self, dwell: Duration) {
        if self.samples.len() >= self.capacity {
            self.samples.pop_front();
        }
        self.samples.push_back(dwell);
    }

    /// Computes the rolling average dwell duration across the recorded samples.
    pub fn average_dwell(&self) -> Duration {
        if self.samples.is_empty() {
            return Duration::ZERO;
        }
        let total: Duration = self.samples.iter().copied().sum();
        total / (self.samples.len() as u32)
    }

    /// Determines if the current rolling average dwell duration exceeds the scaling threshold.
    pub fn should_scale_up(&self, threshold: Duration) -> bool {
        if self.samples.is_empty() {
            return false;
        }
        self.average_dwell() > threshold
    }

    pub fn len(&self) -> usize {
        self.samples.len()
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn clear(&mut self) {
        self.samples.clear();
    }
}

impl Default for QueueDwellMonitor {
    fn default() -> Self {
        Self::new(SLIDING_WINDOW_SIZE)
    }
}
