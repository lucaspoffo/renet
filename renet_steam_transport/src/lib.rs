use std::{sync::mpsc::{self, Receiver}, intrinsics::discriminant_value};

use rechannel::{transport::ClientTransport, server::{RechannelServer, ServerEvent}};
use steamworks::{
    networking_sockets::{InvalidHandle, ListenSocket, NetConnection, NetPollGroup},
    networking_types::{NetworkingConnectionState, NetworkingIdentity, SendFlags, ListenSocketEvent, NetConnectionEnd},
    Client, ClientManager,
};

pub struct SteamClientTransport {
    net_connection: NetConnection<ClientManager>,
}

impl SteamClientTransport {
    pub fn new(server_id: NetworkingIdentity, client: &Client<ClientManager>) -> Self {
        let networking_sockets = client.networking_sockets();
        let net_connection = networking_sockets.connect_p2p(server_id, 0, None).unwrap();

        Self { net_connection }
    }

    pub fn is_connected(&self) -> bool {
        let Ok(info) = self.net_connection.connection_info() else {
            return false;
        };

        let Ok(state) = info.state() else {
            return false;
        };

        state == NetworkingConnectionState::Connected
    }
}

impl ClientTransport for SteamClientTransport {
    fn send(&mut self, packet: &[u8]) {
        if let Err(e) = self.net_connection.send_message(packet, SendFlags::UNRELIABLE) {
            log::error!("Failed to send message to server: {}", e);
        }
    }
}

pub struct SteamServerTransport {
    listen_socket: ListenSocket<ClientManager>,
    poll_group: NetPollGroup<ClientManager>,
}

impl SteamServerTransport {
    pub fn new(client: &Client<ClientManager>) -> Result<Self, InvalidHandle> {
        let networking_sockets = client.networking_sockets();
        let Ok(listen_socket) = networking_sockets.create_listen_socket_p2p(0, None) else {
            return Err(InvalidHandle);
        };

        let poll_group = networking_sockets.create_poll_group();

        Ok(Self { listen_socket, poll_group })
    }

    pub fn update(&mut self, reliable_server: &mut RechannelServer) {
        // Handle disconnected clients from Renet
        for event in reliable_server.events() {
            let ServerEvent::ClientDisconnected { client_id, reason } = event else {
                continue;
            };
            

        }

        while let Some(event) = self.listen_socket.try_receive_event() {
            match event {
                ListenSocketEvent::Connecting(connecting) => {
                    if let Err(e) = connecting.accept() {
                        log::error!("Failed to accept connection: {}", e);
                        connecting.reject(NetConnectionEnd, "");
                    }
                }
                ListenSocketEvent::Connected(connected) => {
                    let client_id = connected.remote().steam_id().unwrap();
                    reliable_server.add_client(client_id.raw());
                    connected.connection().set_poll_group(&self.poll_group);
                }
                ListenSocketEvent::Disconnected(disconnected) => {
                    let client_id = disconnected.remote().steam_id().unwrap();
                    reliable_server.disconnect(client_id.raw());
                }
                
            }
        }
    }
}
