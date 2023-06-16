use steamworks::{
    networking_sockets::{ListenSocket, NetworkingSockets},
    networking_types::{NetworkingConfigEntry, SendFlags},
    networking_utils::NetworkingUtils,
    Client, ClientManager, ServerManager,
};

pub struct Server<Manager = Client> {
    listen_socket: ListenSocket<Manager>,
    //renet_server: NetcodeServer,
}

trait Transport {
    fn update(&self);
    fn send_packets(&self);
}

impl Transport for Server<ClientManager> {
    fn update(&self) {
        match self.listen_socket.try_receive_event() {
            Some(event) => match event {
                steamworks::networking_types::ListenSocketEvent::Connected(event) => {
                    // TODO register client
                    event.connection().send_message(b"Hello, world!", SendFlags::UNRELIABLE);
                }
                steamworks::networking_types::ListenSocketEvent::Disconnected(event) => {
                    // TODO unregister client
                }
                steamworks::networking_types::ListenSocketEvent::Connecting(event) => {
                    // TODO client acceptance check must be done here immediately
                }
            },
            None => {}
        }
    }
    fn send_packets(&self) {}
}

impl Server<ClientManager> {
    pub fn new(client: &Client<ClientManager>) -> Self {
        //  TODO this must be called at the beginning of the application
        client.networking_utils().init_relay_network_access();
        let options: Vec<NetworkingConfigEntry> = Vec::new();
        let socket;
        match client.networking_sockets().create_listen_socket_p2p(0, options) {
            Ok(listen_socket) => {
                socket = listen_socket;
            }
            Err(handle) => {
                panic!("Failed to create listen socket: {:?}", handle);
            }
        }
        // TODO NetcodeServer should be able to deactivate encryption/decryption, because Steam handles that already
        Self {
            listen_socket: socket,
            //renet_server: NetcodeServer::new()
        }
    }

    pub fn update(&mut self) {}
    pub fn send_packets(&mut self) {}
}
