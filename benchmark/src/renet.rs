use alto_logger::TermLogger;
use benchmark::{save_to_csv, save_to_csv_f64, Message, MICROS_PER_FRAME};
use renet::client::Client;
use renet::{
    channel::{ChannelConfig, ReliableOrderedChannelConfig},
    client::{ClientConnected, RequestConnection},
    endpoint::EndpointConfig,
    error::RenetError,
    protocol::unsecure::{UnsecureClientProtocol, UnsecureServerProtocol, UnsecureService},
    server::{Server, ServerConfig},
};
use std::collections::HashMap;
use std::env;
use std::net::UdpSocket;
use std::thread::sleep;
use std::time::{Duration, Instant, SystemTime};

#[derive(Debug, Hash, Eq, PartialEq)]
pub enum Channel {
    Reliable,
}

impl Into<u8> for Channel {
    fn into(self) -> u8 {
        self as u8
    }
}

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

    let mut channel_config = ReliableOrderedChannelConfig::default();
    channel_config.message_resend_time = Duration::from_millis(100);

    let mut channels_config: HashMap<Channel, Box<dyn ChannelConfig>> = HashMap::new();
    channels_config.insert(Channel::Reliable, Box::new(channel_config));

    let endpoint_config = EndpointConfig {
        heartbeat_time: Duration::from_millis(100),
        ..Default::default()
    };

    let mut server: Server<UnsecureServerProtocol, Channel> =
        Server::new(socket, server_config, endpoint_config, channels_config)?;
    let mut tick = 0;
    let mut result: HashMap<u64, f64> = HashMap::new();

    loop {
        if tick > 5000 {
            save_to_csv_f64(result, "renet_server".to_string());
            return Ok(());
        }
        let start = Instant::now();

        server.update();
        if server.has_clients() {
            let message = Message {
                time: SystemTime::now(),
                tick,
            };
            let message = bincode::serialize(&message).expect("Failed to serialize message.");
            let network_info = server.get_client_network_info(0).unwrap();
            result.insert(tick, network_info.sent_bandwidth_kbps);
            //dbg!(network_info);
            server.send_message_to_all_clients(Channel::Reliable, message.into_boxed_slice());
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
    let mut result: HashMap<u64, Duration> = HashMap::new();
    'outer: loop {
        if let Err(e) = connection.process_events() {
            println!("Error processing events: {}", e);
        }

        for payload in connection
            .receive_all_messages_from_channel(Channel::Reliable)
            .iter()
        {
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

        connection.send_packets()?;
    }

    save_to_csv(result, "renet_client".to_string());
    Ok(())
}

fn get_connection(ip: String) -> Result<ClientConnected<UnsecureService, Channel>, RenetError> {
    let socket = UdpSocket::bind("127.0.0.1:8080")?;

    let mut channel_config = ReliableOrderedChannelConfig::default();
    channel_config.message_resend_time = Duration::from_millis(100);

    let mut channels_config: HashMap<Channel, Box<dyn ChannelConfig>> = HashMap::new();
    channels_config.insert(Channel::Reliable, Box::new(channel_config));

    let endpoint_config = EndpointConfig {
        heartbeat_time: Duration::from_millis(100),
        ..Default::default()
    };
    let mut request_connection = RequestConnection::new(
        0,
        socket,
        ip.parse().unwrap(),
        UnsecureClientProtocol::new(0),
        endpoint_config,
        channels_config,
    )?;

    loop {
        if let Some(connection) = request_connection.update()? {
            return Ok(connection);
        }
        sleep(Duration::from_micros(MICROS_PER_FRAME));
    }
}
