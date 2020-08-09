use async_std::io;
use async_std::net::UdpSocket;
use async_std::task;
use renet::{Endpoint, Config};
use alto_logger::TermLogger;
use log::trace;
use std::time::Duration;

fn main() -> io::Result<()> {
    TermLogger::default().init().unwrap();
    task::block_on(async {
        log::set_max_level(log::LevelFilter::max());
        let socket = UdpSocket::bind("127.0.0.1:8081").await?;
        trace!("Listening on {}", socket.local_addr()?);

        let mut buf = vec![7u8; 3500];
        let config = Config::default();
        let mut endpoint = Endpoint::new(config, 0.0, socket);
        
        let msg = "hello world";
        let mut i: u32 = 0;
        loop {
            i.wrapping_add(1);
            if i % 15 == 0 {
                endpoint.update_sent_bandwidth();
            }
            trace!("Sent Bandwidth: {}", endpoint.sent_bandwidth_kbps());
            
            endpoint.send_to(&buf, "127.0.0.1:8080".parse().unwrap()).await.unwrap();
            task::sleep(Duration::from_millis(16)).await;
        }
        // let mut buf = vec![0u8; 1024];
        // let (n, _) = socket.recv_from(&mut buf).await?;
        // println!("-> {}\n", String::from_utf8_lossy(&buf[..n]));
        Ok(())
    })
}
