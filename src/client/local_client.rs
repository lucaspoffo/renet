use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::client::Client;
use crate::error::{DisconnectionReason, MessageError, RenetError};
use crate::packet::Payload;
use crate::remote_connection::{ClientId, NetworkInfo};

use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use log::error;

pub struct LocalClient {
    pub id: u64,
    sender: HashMap<u8, Sender<Payload>>,
    receiver: HashMap<u8, Receiver<Payload>>,
    disconnect_reason: Arc<RwLock<Option<DisconnectionReason>>>,
}

impl LocalClient {
    pub fn is_connected(&self) -> bool {
        self.disconnect_reason.read().unwrap().is_none()
    }

    pub fn disconnect(&mut self) {
        let mut disconnect_reason = self.disconnect_reason.write().unwrap();
        *disconnect_reason = Some(DisconnectionReason::DisconnectedByServer);
    }

    pub fn send_message(&self, channel_id: u8, message: Payload) -> Result<(), MessageError> {
        let channel_sender = self
            .sender
            .get(&channel_id)
            .ok_or(MessageError::InvalidChannel { channel_id })?;
        if let Err(e) = channel_sender.send(message) {
            error!(
                "Error while sending message to local client in channel {}: {}",
                channel_id, e
            );

            let mut disconnect_reason = self.disconnect_reason.write().unwrap();
            *disconnect_reason = Some(DisconnectionReason::ChannelError { channel_id });
        }

        Ok(())
    }

    pub fn receive_message(&self, channel_id: u8) -> Result<Option<Payload>, MessageError> {
        let channel_sender = self
            .receiver
            .get(&channel_id)
            .ok_or(MessageError::InvalidChannel { channel_id })?;
        match channel_sender.try_recv() {
            Ok(payload) => Ok(Some(payload)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(e) => {
                let mut disconnect_reason = self.disconnect_reason.write().unwrap();
                *disconnect_reason = Some(DisconnectionReason::ChannelError { channel_id });
                error!(
                    "Error while receiving message in local client in channel {}: {}",
                    channel_id, e
                );
                Ok(None)
            }
        }
    }
}

pub struct LocalClientConnected {
    id: ClientId,
    sender: HashMap<u8, Sender<Payload>>,
    receiver: HashMap<u8, Receiver<Payload>>,
    network_info: NetworkInfo,
    disconnect_reason: Arc<RwLock<Option<DisconnectionReason>>>,
}

impl LocalClientConnected {
    pub fn new(client_id: u64, channels: Vec<u8>) -> (LocalClientConnected, LocalClient) {
        let mut client_channels_send = HashMap::new();
        let mut host_channels_send = HashMap::new();
        let mut client_channels_recv = HashMap::new();
        let mut host_channels_recv = HashMap::new();

        for &channel in channels.iter() {
            let (channel_send, host_recv) = unbounded();
            client_channels_send.insert(channel, channel_send);
            host_channels_recv.insert(channel, host_recv);

            let (host_send, channel_recv) = unbounded();
            client_channels_recv.insert(channel, channel_recv);
            host_channels_send.insert(channel, host_send);
        }

        let disconnect_reason = Arc::new(RwLock::new(None));

        let host_server = LocalClient {
            disconnect_reason: disconnect_reason.clone(),
            id: client_id,
            sender: host_channels_send,
            receiver: host_channels_recv,
        };

        let host_client = LocalClientConnected {
            disconnect_reason,
            id: client_id,
            sender: client_channels_send,
            receiver: client_channels_recv,
            network_info: NetworkInfo::default(),
        };

        (host_client, host_server)
    }
}

impl Client for LocalClientConnected {
    fn id(&self) -> ClientId {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.disconnect_reason.read().unwrap().is_none()
    }

    fn disconnect(&mut self) {
        let mut disconnect_reason = self.disconnect_reason.write().unwrap();
        *disconnect_reason = Some(DisconnectionReason::DisconnectedByClient);
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        *self.disconnect_reason.read().unwrap()
    }

    fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), MessageError> {
        let sender = self
            .sender
            .get(&channel_id)
            .ok_or(MessageError::InvalidChannel { channel_id })?;
        if let Err(e) = sender.try_send(message) {
            error!(
                "Error while sending message to local connection in channel {}: {}",
                channel_id, e
            );

            let mut disconnect_reason = self.disconnect_reason.write().unwrap();
            *disconnect_reason = Some(DisconnectionReason::ChannelError { channel_id });
        }

        Ok(())
    }

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, MessageError> {
        let receiver = self
            .receiver
            .get(&channel_id)
            .ok_or(MessageError::InvalidChannel { channel_id })?;
        match receiver.try_recv() {
            Ok(payload) => Ok(Some(payload)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(e) => {
                error!(
                    "Error while receiving message in local connection in channel {}: {}",
                    channel_id, e
                );

                let mut disconnect_reason = self.disconnect_reason.write().unwrap();
                *disconnect_reason = Some(DisconnectionReason::ChannelError { channel_id });
                Ok(None)
            }
        }
    }

    fn network_info(&mut self) -> &NetworkInfo {
        &self.network_info
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        Ok(())
    }
}
