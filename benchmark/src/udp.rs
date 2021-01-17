use benchmark::{save_to_csv, save_to_csv_f64, Message, MICROS_PER_FRAME};
use renet::sequence_buffer::SequenceBuffer;
use std::collections::HashMap;
use std::env;
use std::net::{SocketAddr, UdpSocket};
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

fn main() -> std::io::Result<()> {
    let mut args = env::args();
    args.next();
    let command = args.next().expect("Expected server or client argument.");
    let ip = args.next().expect("Expected an ip argument.");
    println!("Command: {}", command);
    if command == "client" {
        client(ip)?;
    } else {
        server(ip)?;
    }
    Ok(())
}

fn server(ip: String) -> std::io::Result<()> {
    println!("Started running server!");
    let socket = UdpSocket::bind("127.0.0.1:5000")?;
    let mut tick = 0;
    let addr: SocketAddr = ip.parse().unwrap();
    let mut sent_packets = SentPackets::new();
    let mut result: HashMap<u64, f64> = HashMap::new();
    loop {
        if tick > 600 {
            save_to_csv_f64(result, "udp_server".to_string());
            return Ok(());
        }
        let start = Instant::now();
        let message = Message {
            time: SystemTime::now(),
            tick,
        };
        println!("{:?}", message);
        let message = bincode::serialize(&message).expect("Failed to serialize message.");

        let sent_packet = SentPacket {
            size: message.len(),
            time: Instant::now(),
        };
        sent_packets.sent_buffer.insert(tick as u16, sent_packet);
        sent_packets.update_sent_bandwidth();
        result.insert(tick, sent_packets.sent_bandwidth_kbps);
        println!("Sent bandwidth kbps: {}", sent_packets.sent_bandwidth_kbps);

        socket.send_to(&message, addr.clone())?;

        let now = Instant::now();
        let frame_duration = Duration::from_micros(MICROS_PER_FRAME);
        if let Some(wait) = (start + frame_duration).checked_duration_since(now) {
            sleep(wait);
        }
        tick += 1;
    }
}

fn client(ip: String) -> std::io::Result<()> {
    println!("Started running client!");
    let socket = UdpSocket::bind(ip)?;
    let mut buf = [0; 16000];
    let mut result: HashMap<u64, Duration> = HashMap::new();
    'outer: loop {
        let (bytes_read, _addr) = socket.recv_from(&mut buf)?;
        if bytes_read > 0 {
            let message = &buf[..bytes_read];

            let message: Message =
                bincode::deserialize(message).expect("Failed to deserialize message.");

            match message.time.elapsed() {
                Ok(delay) => {
                    println!(
                        "Delay from tick {}: {} microseconds",
                        message.tick,
                        delay.as_micros()
                    );

                    result.insert(message.tick, delay);
                }
                Err(e) => {
                    println!("Failed to get elapsed time: {}", e);
                }
            }
            if message.tick == 600 {
                break 'outer;
            }
        }
    }

    save_to_csv(result, "udp".to_string());
    Ok(())
}

#[derive(Debug, Clone)]
struct SentPacket {
    time: Instant,
    size: usize,
}

impl Default for SentPacket {
    fn default() -> Self {
        Self {
            size: 0,
            time: Instant::now(),
        }
    }
}

struct SentPackets {
    pub sent_buffer: SequenceBuffer<SentPacket>,
    pub sent_bandwidth_kbps: f64,
}

impl SentPackets {
    pub fn new() -> Self {
        Self {
            sent_buffer: SequenceBuffer::with_capacity(256),
            sent_bandwidth_kbps: 0.0,
        }
    }

    pub fn update_sent_bandwidth(&mut self) {
        let sample_size = 64;
        let base_sequence = self.sent_buffer.sequence().wrapping_sub(sample_size as u16);

        let mut bytes_sent = 0;
        let mut start_time = Instant::now();
        let mut end_time = Instant::now() - Duration::from_secs(100);
        for i in 0..sample_size {
            if let Some(sent_packet) = self.sent_buffer.get(base_sequence.wrapping_add(i as u16)) {
                if sent_packet.size == 0 {
                    // Only Default Packets have size 0
                    continue;
                }
                bytes_sent += sent_packet.size;
                if sent_packet.time < start_time {
                    start_time = sent_packet.time;
                }
                if sent_packet.time > end_time {
                    end_time = sent_packet.time;
                }
            }
        }

        // Calculate sent bandwidth
        if end_time <= start_time {
            return;
        }

        let sent_bandwidth_kbps =
            bytes_sent as f64 / (end_time - start_time).as_secs_f64() * 8.0 / 1000.0;
        if f64::abs(self.sent_bandwidth_kbps - sent_bandwidth_kbps) > 0.0001 {
            self.sent_bandwidth_kbps += (sent_bandwidth_kbps - self.sent_bandwidth_kbps) * 0.1;
        } else {
            self.sent_bandwidth_kbps = sent_bandwidth_kbps;
        }
    }
}
