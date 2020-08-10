use alto_logger::TermLogger;
use async_std::io;
use async_std::net::UdpSocket;
use async_std::task;
use log::trace;
use renet::{Config, Endpoint};

fn main() -> io::Result<()> {
    TermLogger::default().init().unwrap();
    task::block_on(async {
        let socket = UdpSocket::bind("127.0.0.1:8080").await?;
        println!("Listening on {}", socket.local_addr()?);

        let mut buf = vec![0u8; 1500];
        let config = Config::default();
        let mut endpoint = Endpoint::new(config, socket);
        let mut i: u32 = 0;

        loop {
            if let Ok(Some((packet, addrs))) = endpoint.recv_from(&mut buf).await {
                log::trace!("Received packet with len {} from {}.\n", packet.len(), addrs);
                endpoint.send_to(&packet, addrs).await;
            }
            i = i.wrapping_add(1);
            if i % 15 == 0 {
                endpoint.update_received_bandwidth();
            }
            trace!(
                "Received Bandwidth: {} kbps",
                endpoint.received_bandwidth_kbps()
            );
            trace!("RTT: {}", endpoint.rtt()); 
            //let sent = socket.send_to(&buf[..n], &peer).await?;
            //println!("Sent {} out of {} bytes to {}", sent, n , peer);
        }
    })
}
