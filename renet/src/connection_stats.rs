use std::time::Duration;

const RESOLUTION: Duration = Duration::from_millis(300);
const WINDOW: Duration = Duration::from_millis(6000);
const SIZE: usize = (WINDOW.as_millis() / RESOLUTION.as_millis()) as usize;

#[derive(Debug, Default)]
pub struct ConnectionStats {
    packets_sent: [u64; SIZE],
    packets_acked: [u64; SIZE],
    bytes_sent: [u64; SIZE],
    bytes_received: [u64; SIZE],
    current_index: usize,
}

impl ConnectionStats {
    pub fn new() -> Self {
        Self {
            packets_sent: [0; SIZE],
            packets_acked: [0; SIZE],
            bytes_sent: [0; SIZE],
            bytes_received: [0; SIZE],
            current_index: 0,
        }
    }

    fn index(time: Duration) -> usize {
        (time.as_millis() / RESOLUTION.as_millis()) as usize % SIZE
    }

    pub fn update(&mut self, current_time: Duration) {
        let i = Self::index(current_time);
        if self.current_index != i {
            self.current_index = i;
            self.packets_sent[i] = 0;
            self.bytes_sent[i] = 0;
            self.bytes_received[i] = 0;
            self.packets_acked[i] = 0;
        }
    }

    pub fn sent_packets(&mut self, num_packets: u64, bytes: u64) {
        self.packets_sent[self.current_index] += num_packets;
        self.bytes_sent[self.current_index] += bytes;
    }

    pub fn received_packet(&mut self, bytes: u64) {
        self.bytes_received[self.current_index] += bytes;
    }

    pub fn acked_packet(&mut self, sent_at: Duration, current_time: Duration) {
        let delta = current_time - sent_at;
        if delta > WINDOW {
            // Out of the duration window, discard it
            return;
        }

        self.packets_acked[Self::index(sent_at)] += 1;
    }

    pub fn bytes_sent_per_second(&self, current_time: Duration) -> f64 {
        let mut total_bytes: u64 = self.bytes_sent.iter().sum();

        if current_time < WINDOW {
            return total_bytes as f64 / current_time.as_secs_f64();
        }

        // Ignore the current incomplete resolution
        total_bytes -= self.bytes_sent[self.current_index];

        total_bytes as f64 / (WINDOW - RESOLUTION).as_secs_f64()
    }

    pub fn bytes_received_per_second(&self, current_time: Duration) -> f64 {
        let mut total_bytes: u64 = self.bytes_received.iter().sum();

        if current_time < WINDOW {
            return total_bytes as f64 / current_time.as_secs_f64();
        }

        // Ignore the current incomplete resolution
        total_bytes -= self.bytes_received[self.current_index];
        total_bytes as f64 / (WINDOW - RESOLUTION).as_secs_f64()
    }

    pub fn packet_loss(&self) -> f64 {
        let total_packets_sent = {
            let mut sum: u64 = self.packets_sent.iter().sum();

            // Ignore the current and last 2 resolutions,
            // because the message or its ack could be in flight
            sum -= self.packets_sent[self.current_index];
            sum -= self.packets_sent[(self.current_index + SIZE - 1) % SIZE];
            sum -= self.packets_sent[(self.current_index + SIZE - 2) % SIZE];
            sum as f64
        };

        let total_packets_acked = {
            let mut sum: u64 = self.packets_acked.iter().sum();
            sum -= self.packets_acked[self.current_index];
            sum -= self.packets_acked[(self.current_index + SIZE - 1) % SIZE];
            sum -= self.packets_acked[(self.current_index + SIZE - 2) % SIZE];
            sum as f64
        };

        if total_packets_sent == 0.0 {
            return 0.0;
        }

        (total_packets_sent - total_packets_acked) / total_packets_sent
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_per_sec() {
        let mut current_time = Duration::ZERO;
        let mut window = ConnectionStats::default();

        for _ in 0..10 {
            window.update(current_time);
            window.sent_packets(10, 100);
            current_time += Duration::from_millis(100);
        }

        // Check at 1 second
        assert_eq!(window.bytes_sent_per_second(current_time), 1000.);

        for _ in 0..50 {
            window.update(current_time);
            window.sent_packets(10, 100);
            current_time += Duration::from_millis(100);
        }

        // Check after 6 seconds
        assert_eq!(window.packets_sent, [30; SIZE]);
        assert_eq!(window.bytes_sent, [300; SIZE]);
        assert_eq!(window.bytes_sent_per_second(current_time), 1000.);
    }

    #[test]
    fn packet_loss() {
        let mut current_time = Duration::ZERO;
        let mut window = ConnectionStats::default();

        for _ in 0..20 {
            window.update(current_time);
            // Send 2, ack only 1
            window.sent_packets(2, 100);
            window.acked_packet(current_time, current_time);
            current_time += Duration::from_millis(100);
        }

        // Check at 2 second
        assert_eq!(window.packet_loss(), 0.5);

        for _ in 0..40 {
            window.update(current_time);
            window.sent_packets(2, 100);
            window.acked_packet(current_time, current_time);
            current_time += Duration::from_millis(100);
        }

        // Check after 6 seconds
        assert_eq!(window.packets_sent, [6; SIZE]);
        assert_eq!(window.packets_acked, [3; SIZE]);
        assert_eq!(window.packet_loss(), 0.5);
    }
}
