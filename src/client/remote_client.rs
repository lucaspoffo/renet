use crate::channel::ChannelConfig;
use crate::client::Client;
use crate::error::{DisconnectionReason, MessageError, RenetError};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};
use crate::transport::TransportClient;
use crate::ClientId;

use std::collections::HashMap;

pub struct RemoteClient<C, T> {
    id: C,
    transport: T,
    connection: RemoteConnection<C>,
}

impl<C, T> RemoteClient<C, T>
where
    C: ClientId,
    T: TransportClient,
{
    pub fn new(
        id: C,
        transport: T,
        channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
        connection_config: ConnectionConfig,
    ) -> Self {
        let mut connection = RemoteConnection::new(id, connection_config);

        for (channel_id, channel_config) in channels_config.iter() {
            let channel = channel_config.new_channel();
            connection.add_channel(*channel_id, channel);
        }

        Self {
            id,
            transport,
            connection,
        }
    }
}

impl<C, T> Client<C> for RemoteClient<C, T>
where
    C: ClientId,
    T: TransportClient,
{
    fn id(&self) -> C {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.transport.is_connected() && self.connection.is_connected()
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.connection
            .connection_error()
            .or_else(|| self.transport.connection_error())
    }

    fn disconnect(&mut self) {
        self.connection
            .disconnect(DisconnectionReason::DisconnectedByClient);
        self.transport
            .disconnect(DisconnectionReason::DisconnectedByClient);
    }

    fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), MessageError> {
        self.connection.send_message(channel_id, message)
    }

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, MessageError> {
        self.connection.receive_message(channel_id)
    }

    fn network_info(&self) -> &NetworkInfo {
        self.connection.network_info()
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        let packets = self.connection.get_packets_to_send()?;
        for packet in packets.into_iter() {
            self.transport.send_to_server(&packet);
        }
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        if let Some(connection_error) = self.connection_error() {
            return Err(RenetError::ConnectionError(connection_error));
        }

        while let Some(payload) = self.transport.recv()? {
            self.connection.process_payload(&payload)?;
        }

        self.transport.update();
        self.connection.update()
    }
}
