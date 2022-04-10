use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Timer {
    duration: Duration,
    elapsed: Duration,
}

impl Timer {
    pub fn new(duration: Duration) -> Self {
        Timer { elapsed: Duration::ZERO, duration }
    }

    pub fn reset(&mut self) {
        self.elapsed = Duration::ZERO;
    }

    pub fn advance(&mut self, duration: Duration) {
        self.elapsed += duration;
    }

    pub fn finish(&mut self) {
        self.elapsed = self.duration;
    }

    pub fn is_finished(&self) -> bool {
        self.elapsed >= self.duration
    }
}
