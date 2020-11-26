use benchmark::{save_to_csv, Message, MICROS_PER_FRAME};
use std::collections::HashMap;
use std::env;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
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
    let listener = TcpListener::bind(ip)?;

    for stream in listener.incoming() {
        handle_client(stream?)?;
    }

    Ok(())
}

fn handle_client(mut stream: TcpStream) -> std::io::Result<()> {
    let mut tick = 0;
    stream.set_nodelay(true)?;
    loop {
        if tick > 600 {
            return Ok(());
        }
        let start = Instant::now();
        let message = Message {
            time: SystemTime::now(),
            tick,
        };
        println!("{:?}", message);
        let message = bincode::serialize(&message).expect("Failed to serialize message.");
        stream.write(&message)?;

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
    let mut stream = TcpStream::connect(ip)?;
    println!("TTL: {}", stream.ttl().unwrap());
    let mut buf = [0; 16000];
    let mut result: HashMap<u64, Duration> = HashMap::new();
    'outer: loop {
        let bytes_read = stream.read(&mut buf)?;
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

    save_to_csv(result, "tcp".to_string());
    Ok(())
}
