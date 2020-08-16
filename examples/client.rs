use alto_logger::TermLogger;
use log::trace;
use renet::{Config, Endpoint};
use std::net::UdpSocket;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    TermLogger::default().init().unwrap();
    log::set_max_level(log::LevelFilter::max());
    let socket = UdpSocket::bind("127.0.0.1:8081")?;
    trace!("Listening on {}", socket.local_addr()?);

    let payload = vec![7u8; 3500];
    let mut buf = vec![0u8; 1500];
    let config = Config::default();
    let mut endpoint = Endpoint::new(config);

    let mut i: u32 = 0;
    loop {
        i = i.wrapping_add(1);
        if i % 15 == 0 {
            endpoint.update_sent_bandwidth();
            endpoint.update_received_bandwidth();
        }
        trace!("Sent Bandwidth: {}", endpoint.sent_bandwidth_kbps());
        trace!("Receive Bandwidth: {}", endpoint.received_bandwidth_kbps());
        trace!("RTT: {}", endpoint.rtt());
        trace!("Packet Loss: {}%", endpoint.packet_loss());
        endpoint
            .send_to(&payload, "127.0.0.1:8080".parse().unwrap(), &socket)
            .unwrap();
        if let Ok(Some((packet, addrs))) = endpoint.recv_from(&mut buf, &socket) {
            log::trace!("Received packet with len {}\n from {}", packet.len(), addrs);
        }
        //std::thread::sleep(Duration::from_millis(16));
    }
}
