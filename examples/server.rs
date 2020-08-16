use alto_logger::TermLogger;
use log::trace;
use renet::{Config, Endpoint};
use std::net::UdpSocket;

fn main() -> std::io::Result<()> {
    TermLogger::default().init().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:8080")?;
    println!("Listening on {}", socket.local_addr()?);

    let mut buf = vec![0u8; 1500];
    let config = Config::default();
    let mut endpoint = Endpoint::new(config);
    let mut i: u32 = 0;

    loop {
        if let Ok(Some((packet, addrs))) = endpoint.recv_from(&mut buf, &socket) {
            log::trace!(
                "Received packet with len {} from {}.\n",
                packet.len(),
                addrs
            );
            endpoint.send_to(&packet, addrs, &socket);
        }
        i = i.wrapping_add(1);
        if i % 15 == 0 {
            endpoint.update_received_bandwidth();
            endpoint.update_sent_bandwidth();
        }
        trace!("Received Bandwidth: {}", endpoint.received_bandwidth_kbps());
        trace!("Sent Bandwidth: {}", endpoint.sent_bandwidth_kbps());
        trace!("RTT: {}", endpoint.rtt());
        trace!("Packet Loss: {}%", endpoint.packet_loss());
        //let sent = socket.send_to(&buf[..n], &peer).await?;
        //println!("Sent {} out of {} bytes to {}", sent, n , peer);
    }
}
