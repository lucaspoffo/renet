use std::{
    sync::{Arc, RwLock},
    time::Duration,
};

use matcher::{RegisterServer, ServerUpdate};
use reqwest::blocking::Client;

pub enum Status {
    Registering,
    Updating { server_id: u64 },
}

pub fn update_lobby_status(mut register_server: RegisterServer, server_update: Arc<RwLock<ServerUpdate>>) {
    let client = Client::new();
    let mut status = Status::Registering;
    loop {
        if Arc::strong_count(&server_update) == 1 {
            // The server dropped so we can return
            log::info!("Stopped updating lobby status");
            if let Status::Updating { server_id } = status {
                if let Err(e) = remove_server_request(server_id, &client) {
                    log::error!("Failed to remove server from listing: {}", e);
                }
            }
            return;
        }

        match status {
            Status::Registering => {
                register_server.current_clients = server_update.read().unwrap().current_clients;
                match register_server_request(&register_server, &client) {
                    Err(e) => {
                        log::error!("Failed to register server: {}", e);
                    }
                    Ok(server_id) => {
                        status = Status::Updating { server_id };
                    }
                }
            }
            Status::Updating { server_id } => {
                let server_update = server_update.read().unwrap().clone();
                if let Err(e) = update_server_request(server_id, &server_update, &client) {
                    log::error!("Failed to update server: {}", e);
                    status = Status::Registering;
                }
            }
        }

        std::thread::sleep(Duration::from_secs(5));
    }
}

fn update_server_request(server_id: u64, server_update: &ServerUpdate, client: &Client) -> Result<(), reqwest::Error> {
    let res = client
        .put(format!("http://127.0.0.1:7000/server/{server_id}"))
        .json(server_update)
        .send()?;
    res.error_for_status()?;
    Ok(())
}

fn register_server_request(register_server: &RegisterServer, client: &Client) -> Result<u64, reqwest::Error> {
    let res = client.post("http://127.0.0.1:7000/server").json(&register_server).send()?;
    res.error_for_status_ref()?;
    let server_id: u64 = res.json()?;
    Ok(server_id)
}

fn remove_server_request(server_id: u64, client: &Client) -> Result<(), reqwest::Error> {
    let res = client.delete(format!("http://127.0.0.1:7000/server/{server_id}")).send()?;
    res.error_for_status()?;
    Ok(())
}
