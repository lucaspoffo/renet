use benchmark::{Message, MICROS_PER_FRAME, save_to_csv_f64, save_to_csv};
use renet::{
    channel::{ChannelConfig, ReliableOrderedChannel},
    server::{Server, ServerConfig},
    client::ClientConnected,
    connection::RequestConnection,
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol},
    error::RenetError
};
use std::env;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};
use std::collections::HashMap;
use alto_logger::TermLogger;

fn main() -> Result<(), RenetError> {
    TermLogger::default().init().unwrap();

    let mut args = env::args();
    args.next();
    let command = args.next().expect("Expected server or client argument.");
    let ip = args.next().expect("Expected an ip argument.");
    println!("Command: {}", command);
    println!("IP: {}", ip);
    if command == "client" {
        client(ip)?;
    } else {
        server(ip)?;
    }
    Ok(())
}

fn server(ip: String) -> Result<(), RenetError> {
    let socket = UdpSocket::bind(ip)?;
    let server_config = ServerConfig::default();
    let mut server: Server<UnsecureServerProtocol> = Server::new(socket, server_config)?;
    let mut tick = 0;
    let mut channel_config = ChannelConfig::default();
    channel_config.message_resend_time = Duration::from_millis(100);
    server.add_channel_config(0, channel_config);
    let mut result: HashMap<u64, f64> = HashMap::new();

    loop {
        if tick > 600 {
            save_to_csv_f64(result, "renet_server".to_string());
            return Ok(());
        }
        let start = Instant::now();

        server.update(start.clone());
        if server.has_clients() {
            let message = Message { time: SystemTime::now(), tick };
            let message = bincode::serialize(&message).expect("Failed to serialize message.");
            let network_info = server.get_client_network_info(0).unwrap();
            result.insert(tick, network_info.sent_bandwidth_kbps);
            //dbg!(network_info);
            server.send_message_to_all_clients(0, message.into_boxed_slice());
            server.send_packets();
            tick += 1;
        }

        let now = Instant::now();
        let frame_duration = Duration::from_micros(MICROS_PER_FRAME);
        if let Some(wait) = (start + frame_duration).checked_duration_since(now) {
            sleep(wait);
        }
    }
}

fn client(ip: String) -> Result<(), RenetError> {
    let mut connection = get_connection(ip)?;
    let channel_config = ChannelConfig::default();
    let channel = ReliableOrderedChannel::new(Instant::now(), channel_config);
    connection.add_channel(0, Box::new(channel));
    let mut received_message;
    let mut result: HashMap<u64, Duration> = HashMap::new();
    let mut count = 0;
    'outer: loop {
        count += 1;
        received_message = false;
        connection.process_events(Instant::now())?;

        for payload in connection.receive_all_messages_from_channel(0).iter() {
            received_message = true;
            let message: Message =
                bincode::deserialize(payload).expect("Failed to deserialize message.");

            let delay = SystemTime::now().duration_since(message.time).unwrap();
            result.insert(message.tick, delay);
            println!(
                "Delay from tick {}: {} microseconds",
                message.tick,
                delay.as_micros()
            );
            if message.tick == 600 {
                break 'outer;
            }
        }
        if received_message || count > 5000 {
            count = 0;
            connection.send_message(0, vec![0u8].into_boxed_slice());
        }

        connection.send_packets()?;
    }

    save_to_csv(result, "renet_client".to_string());
    Ok(())
}

fn get_connection(ip: String) -> Result<ClientConnected, RenetError> {
    let socket = UdpSocket::bind("127.0.0.1:8080")?;
    let mut request_connection =
        RequestConnection::new(0, socket, ip.parse().unwrap(), Box::new(UnsecureClientProtocol::new(0)))?;
    loop {
        if let Some(connection) = request_connection.update()? {
                return Ok(connection);
        }
        sleep(Duration::from_micros(MICROS_PER_FRAME));
    }
}
