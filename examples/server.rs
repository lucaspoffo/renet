use alto_logger::TermLogger;
use log::trace;
use renet::{Endpoint, EndpointConfig};
use std::net::UdpSocket;

fn main() -> std::io::Result<()> {
    TermLogger::default().init().unwrap();
    let socket = UdpSocket::bind("127.0.0.1:8080")?;
    println!("Listening on {}", socket.local_addr()?);

    let mut buf = vec![0u8; 1500];
    let config = EndpointConfig::default();
    let mut endpoint = Endpoint::new(config);
    let mut i: u32 = 0;

    loop {
        if let Ok(Some((packet, addrs))) = endpoint.recv_from(&mut buf, &socket) {
            log::trace!(
                "Received packet with len {} from {}.\n",
                packet.len(),
                addrs
            );
            if let Err(e) = endpoint.send_to(&packet, addrs, &socket) {
                log::error!("Error when sending packet to {}: {:?}", addrs, e);
            }
        }
        i = i.wrapping_add(1);
        if i % 15 == 0 {
            endpoint.update_received_bandwidth();
            endpoint.update_sent_bandwidth();
        }
        let network_info = endpoint.network_info();
        trace!(
            "Received Bandwidth: {}",
            network_info.received_bandwidth_kbps
        );
        trace!("Sent Bandwidth: {}", network_info.sent_bandwidth_kbps);
        trace!("RTT: {}", network_info.rtt);
        trace!("Packet Loss: {}%", network_info.packet_loss);
        //let sent = socket.send_to(&buf[..n], &peer).await?;
        //println!("Sent {} out of {} bytes to {}", sent, n , peer);
    }
}
