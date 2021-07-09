use std::collections::HashMap;

use crate::client::Client;
use crate::error::RenetError;
use crate::remote_connection::{ClientId, NetworkInfo};

use crossbeam_channel::{unbounded, Receiver, Sender};

pub struct LocalClient {
    pub id: u64,
    sender: HashMap<u8, Sender<Box<[u8]>>>,
    receiver: HashMap<u8, Receiver<Box<[u8]>>>,
}

impl LocalClient {
    pub fn send_message(&self, channel_id: u8, message: Box<[u8]>) -> Result<(), RenetError> {
        let channel_sender = self
            .sender
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        // TODO: handle TrySendError
        channel_sender.send(message).unwrap();
        Ok(())
    }

    pub fn receive_message(&self, channel_id: u8) -> Result<Option<Box<[u8]>>, RenetError> {
        let channel_sender = self
            .receiver
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        // TODO: handle TryRecvError
        Ok(channel_sender.try_recv().ok())
    }
}

pub struct LocalClientConnected {
    id: ClientId,
    sender: HashMap<u8, Sender<Box<[u8]>>>,
    receiver: HashMap<u8, Receiver<Box<[u8]>>>,
    network_info: NetworkInfo,
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

        let host_server = LocalClient {
            id: client_id,
            sender: host_channels_send,
            receiver: host_channels_recv,
        };

        let host_client = LocalClientConnected {
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
        true
    }

    fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) -> Result<(), RenetError> {
        let sender = self
            .sender
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        // TODO: handle TrySendError
        sender.try_send(message).unwrap();
        Ok(())
    }

    fn receive_message(&mut self, channel_id: u8) -> Result<Option<Box<[u8]>>, RenetError> {
        let receiver = self
            .receiver
            .get(&channel_id)
            .ok_or(RenetError::InvalidChannel { channel_id })?;
        // TODO: handle TryRecvError
        Ok(receiver.try_recv().ok())
    }

    fn network_info(&mut self) -> &NetworkInfo {
        &self.network_info
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        Ok(())
    }

    fn process_events(&mut self) -> Result<(), RenetError> {
        Ok(())
    }
}
