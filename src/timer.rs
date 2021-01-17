use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct Timer {
    duration: Duration,
    start: Instant,
}

impl Timer {
    pub fn new(duration: Duration) -> Self {
        Timer {
            start: Instant::now(),
            duration,
        }
    }

    pub fn reset(&mut self) {
        self.start = Instant::now();
    }

    pub fn is_finished(&self) -> bool {
        Instant::now().saturating_duration_since(self.start) > self.duration
    }
}
