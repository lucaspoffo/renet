use async_std::io;
use async_std::net::UdpSocket;
use async_std::task;
use renet::{Endpoint, Config};

fn main() -> io::Result<()> {
    task::block_on(async {
        let socket = UdpSocket::bind("127.0.0.1:8081").await?;
        println!("Listening on {}", socket.local_addr()?);

        let mut buf = vec![7u8; 1500];
        let config = Config::default();
        let mut endpoint = Endpoint::new(config, 0.0, socket);
        
        let msg = "hello world";
        println!("<- {}", msg);
        loop {

            endpoint.send_to(&buf, "127.0.0.1:8080".parse().unwrap()).await.unwrap();
        }
        // let mut buf = vec![0u8; 1024];
        // let (n, _) = socket.recv_from(&mut buf).await?;
        // println!("-> {}\n", String::from_utf8_lossy(&buf[..n]));
        Ok(())
    })
}
