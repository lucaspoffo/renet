use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{atomic::AtomicU64, RwLock},
    time::{Duration, Instant, SystemTime},
};

use actix_rt::{spawn, time::interval};
use actix_web::{
    delete, get, middleware, post, put,
    web::{self, Data, Json},
    App, HttpResponse, HttpServer,
};

use renet::{ConnectToken, NETCODE_KEY_BYTES};

use matcher::{LobbyListing, RegisterServer, RequestConnection, ServerUpdate, Username, PROTOCOL_ID};

struct Server {
    name: String,
    current_clients: u64,
    max_clients: u64,
    last_updated: Instant,
    address: SocketAddr,
    password: Option<String>,
    private_key: [u8; NETCODE_KEY_BYTES],
}

impl From<RegisterServer> for Server {
    fn from(register_server: RegisterServer) -> Self {
        Self {
            name: register_server.name.clone(),
            address: register_server.address,
            current_clients: register_server.current_clients,
            max_clients: register_server.max_clients,
            private_key: register_server.private_key,
            password: register_server.password,
            last_updated: Instant::now(),
        }
    }
}

#[derive(Default)]
struct LobbyList {
    servers: RwLock<HashMap<u64, Server>>,
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    env_logger::init();
    println!("starting HTTP server at http://localhost:7000");

    let lobby_list = Data::new(LobbyList::default());
    let server_id = Data::new(AtomicU64::new(0));

    let lobby_list_clone = lobby_list.clone();
    spawn(async move {
        cleanup_server_list(lobby_list_clone).await;
    });

    HttpServer::new(move || {
        App::new()
            .wrap(middleware::Logger::default())
            .app_data(lobby_list.clone())
            .app_data(server_id.clone())
            .service(server_register)
            .service(server_connect)
            .service(server_list)
            .service(server_update)
            .service(server_remove)
    })
    .bind(("127.0.0.1", 7000))?
    .run()
    .await
}

async fn cleanup_server_list(lobby_list: Data<LobbyList>) {
    let mut interval = interval(Duration::from_secs(10));
    loop {
        interval.tick().await;

        let mut servers = lobby_list.servers.write().unwrap();
        servers.retain(|_, lobby| lobby.last_updated.elapsed() < Duration::from_secs(15));
    }
}

#[post("/server")]
async fn server_register(server_id: Data<AtomicU64>, lobby_list: Data<LobbyList>, info: Json<RegisterServer>) -> HttpResponse {
    let new_server_id: u64 = server_id.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

    let mut servers = lobby_list.servers.write().unwrap();
    let server: Server = info.into_inner().into();
    servers.insert(new_server_id, server);

    HttpResponse::Ok().json(new_server_id)
}

#[put("/server/{server_id}")]
async fn server_update(path: web::Path<u64>, lobby_list: Data<LobbyList>, info: Json<ServerUpdate>) -> HttpResponse {
    let mut servers = lobby_list.servers.write().unwrap();
    let server_id = path.into_inner();

    match servers.get_mut(&server_id) {
        Some(server) => {
            server.last_updated = Instant::now();
            server.current_clients = info.current_clients;
            server.max_clients = info.max_clients;

            HttpResponse::Ok().finish()
        }
        None => HttpResponse::NotFound().finish(),
    }
}

#[delete("/server/{server_id}")]
async fn server_remove(path: web::Path<u64>, lobby_list: Data<LobbyList>) -> HttpResponse {
    let mut servers = lobby_list.servers.write().unwrap();
    let server_id = path.into_inner();

    match servers.remove(&server_id) {
        Some(_) => HttpResponse::Ok().finish(),
        None => HttpResponse::NotFound().finish(),
    }
}

#[post("/server/{server_id}/connect")]
async fn server_connect(path: web::Path<u64>, lobby_list: Data<LobbyList>, request_connection: Json<RequestConnection>) -> HttpResponse {
    let servers = lobby_list.servers.read().unwrap();
    let server_id = path.into_inner();

    match servers.get(&server_id) {
        Some(server) => {
            if server.password != request_connection.password {
                return HttpResponse::Unauthorized().finish();
            }
            let current_time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap();
            let user_data = Username(request_connection.username.clone()).to_netcode_user_data();
            let client_id = current_time.as_millis() as u64;
            let connect_token = ConnectToken::generate(
                current_time,
                PROTOCOL_ID,
                300,
                client_id,
                15,
                vec![server.address],
                Some(&user_data),
                &server.private_key,
            )
            .unwrap();
            let mut bytes = Vec::new();
            connect_token.write(&mut bytes).unwrap();
            HttpResponse::Ok().body(bytes)
        }
        None => HttpResponse::NotFound().finish(),
    }
}

#[get("/server")]
async fn server_list(server_list: Data<LobbyList>) -> HttpResponse {
    let servers = server_list.servers.read().unwrap();
    let simple_servers: Vec<LobbyListing> = servers
        .iter()
        .map(|(key, server)| LobbyListing {
            id: *key,
            name: server.name.clone(),
            max_clients: server.max_clients,
            current_clients: server.current_clients,
            is_protected: server.password.is_some(),
        })
        .collect();
    let body = serde_json::to_string(&simple_servers).unwrap();
    HttpResponse::Ok().body(body)
}
