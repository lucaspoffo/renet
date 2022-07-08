use std::time::Duration;

#[derive(Debug, Clone, Default)]
pub struct Timer {
    duration: Duration,
    start_time: Duration,
    force_finish: bool,
}

impl Timer {
    pub fn new(start_time: Duration, duration: Duration) -> Self {
        Timer {
            duration,
            start_time,
            force_finish: false,
        }
    }

    pub fn reset(&mut self, current_time: Duration) {
        self.start_time = current_time;
        self.force_finish = false;
    }

    pub fn finish(&mut self) {
        self.force_finish = true;
    }

    pub fn is_finished(&self, current_time: Duration) -> bool {
        self.force_finish || (current_time - self.start_time >= self.duration)
    }
}
