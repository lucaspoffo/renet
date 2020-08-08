use async_std::io;
use async_std::net::UdpSocket;
use async_std::task;
use renet::{Endpoint, Config};
use alto_logger::TermLogger;

fn main() -> io::Result<()> {
    TermLogger::default().init().unwrap();
    task::block_on(async {

        let socket = UdpSocket::bind("127.0.0.1:8080").await?;
        println!("Listening on {}", socket.local_addr()?);

        let mut buf = vec![0u8; 1500];
        let config = Config::default();
        let mut endpoint = Endpoint::new(config, 0.0, socket);

        loop {
            if let Ok(Some(packet)) = endpoint.receive(&mut buf).await {
                log::trace!("Received packet with len {}:\n", packet.len());
            }
            //let sent = socket.send_to(&buf[..n], &peer).await?;
            //println!("Sent {} out of {} bytes to {}", sent, n , peer);
        }
    })
}
