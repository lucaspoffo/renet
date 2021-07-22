use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use crate::client::Client;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::Payload;
use crate::remote_connection::{ClientId, NetworkInfo};

use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};

pub struct LocalClient {
    pub id: u64,
    sender: HashMap<u8, Sender<Payload>>,
    receiver: HashMap<u8, Receiver<Payload>>,
    connected: Arc<AtomicBool>,
}

impl LocalClient {
    pub fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    pub fn disconnect(&mut self) {
        self.connected.store(false, Ordering::Relaxed);
    }

    pub fn send_message(&self, channel_id: u8, message: Payload) -> Result<(), RenetError> {
        let channel_sender = self
            .sender
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        channel_sender
            .send(message)
            .map_err(|e| RenetError::ChannelError(Box::new(e)))
    }

    pub fn receive_message(&self, channel_id: u8) -> Result<Option<Payload>, RenetError> {
        let channel_sender = self
            .receiver
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        match channel_sender.try_recv() {
            Ok(payload) => Ok(Some(payload)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(e) => Err(RenetError::ChannelError(Box::new(e))),
        }
    }
}

pub struct LocalClientConnected {
    id: ClientId,
    sender: HashMap<u8, Sender<Payload>>,
    receiver: HashMap<u8, Receiver<Payload>>,
    network_info: NetworkInfo,
    connected: Arc<AtomicBool>,
    disconnect_reason: Option<DisconnectionReason>,
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

        let connected = Arc::new(AtomicBool::new(true));

        let host_server = LocalClient {
            connected: connected.clone(),
            id: client_id,
            sender: host_channels_send,
            receiver: host_channels_recv,
        };

        let host_client = LocalClientConnected {
            connected,
            id: client_id,
            sender: client_channels_send,
            receiver: client_channels_recv,
            network_info: NetworkInfo::default(),
            disconnect_reason: None,
        };

        (host_client, host_server)
    }
}

impl Client for LocalClientConnected {
    fn id(&self) -> ClientId {
        self.id
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Relaxed)
    }

    fn disconnect(&mut self) {
        self.disconnect_reason = Some(DisconnectionReason::DisconnectedByClient);
        self.connected.store(false, Ordering::Relaxed);
    }

    fn connection_error(&self) -> Option<DisconnectionReason> {
        self.disconnect_reason
    }

    fn send_message(&mut self, channel_id: u8, message: Payload) -> Result<(), RenetError> {
        let sender = self
            .sender
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        sender
            .try_send(message)
            .map_err(|e| RenetError::ChannelError(Box::new(e)))
    }

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Payload>, RenetError> {
        let receiver = self
            .receiver
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        match receiver.try_recv() {
            Ok(payload) => Ok(Some(payload)),
            Err(TryRecvError::Empty) => Ok(None),
            Err(e) => Err(RenetError::ChannelError(Box::new(e))),
        }
    }

    fn network_info(&mut self) -> &NetworkInfo {
        &self.network_info
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        if !self.is_connected() && self.disconnect_reason.is_none() {
            self.disconnect_reason = Some(DisconnectionReason::DisconnectedByServer);
        }
        Ok(())
    }
}
