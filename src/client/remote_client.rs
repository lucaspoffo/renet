use crate::channel::ChannelConfig;
use crate::client::Client;
use crate::error::{DisconnectionReason, MessageError, RenetError};
use crate::packet::Payload;
use crate::remote_connection::{ConnectionConfig, NetworkInfo, RemoteConnection};
use crate::{ClientId, ConnectionControl, Transport};

use std::collections::HashMap;

pub struct RemoteClient<C, T> {
    transport: T,
    id: C,
    connection: RemoteConnection<C>,
    connection_control: ConnectionControl<C>,
}

impl<C, T> RemoteClient<C, T>
where
    C: ClientId,
    T: Transport + Transport<ClientId = C>,
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

        // TODO: how we will impl Transporft for UdpClient without connection_control
        let connection_control = ConnectionControl::new(crate::server::ConnectionPermission::All);

        Self {
            connection_control,
            id,
            connection,
            transport,
        }
    }
}

impl<C, T> Client<C> for RemoteClient<C, T>
where
    C: ClientId,
    T: Transport + Transport<ClientId = C>,
{
    fn id(&self) -> C {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.connection.is_connected()
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.connection.connection_error()
    }

    fn disconnect(&mut self) {
        self.connection.disconnect(&mut self.transport);
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
        self.connection.send_packets(&mut self.transport)?;
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        if let Some(connection_error) = self.connection_error() {
            return Err(RenetError::ConnectionError(connection_error));
        }

        // TODO: How to validate that it's only server payload
        // Add validation to transport layer?
        // Ex: UdpClient, client.server_addr, validate that only we can receive from there
        while let Ok(Some((_, payload))) = self.transport.recv(&self.connection_control) {
            self.connection.process_payload(&payload)?;
        }
        self.connection.update()
    }
}
