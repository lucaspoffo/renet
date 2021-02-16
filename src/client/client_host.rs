use std::collections::HashMap;
use std::time::Instant;

use crate::client::Client;
use crate::error::RenetError;
use crate::{connection::ClientId, endpoint::NetworkInfo};

use crossbeam_channel::{unbounded, Receiver, Sender};

pub struct HostServer {
    pub id: u64,
    sender: HashMap<u8, Sender<Box<[u8]>>>,
    receiver: HashMap<u8, Receiver<Box<[u8]>>>,
}

impl HostServer {
    pub fn send_message(&self, channel_id: u8, message: Box<[u8]>) {
        let channel_sender = self.sender.get(&channel_id).unwrap();
        channel_sender.send(message).unwrap();
    }

    pub fn receive_messages(&self, channel_id: u8) -> Option<Vec<Box<[u8]>>> {
        let mut messages = vec![];
        let channel_sender = self.receiver.get(&channel_id).unwrap();
        while let Ok(message) = channel_sender.try_recv() {
            messages.push(message);
        }
        if messages.is_empty() {
            return None;
        }
        Some(messages)
    }
}

pub struct HostClient {
    id: ClientId,
    sender: HashMap<u8, Sender<Box<[u8]>>>,
    receiver: HashMap<u8, Receiver<Box<[u8]>>>,
    network_info: NetworkInfo,
}

impl HostClient {
    pub fn new(client_id: u64, channels: Vec<u8>) -> (HostClient, HostServer) {
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

        let host_server = HostServer {
            id: client_id,
            sender: host_channels_send,
            receiver: host_channels_recv,
        };

        let host_client = HostClient {
            id: client_id,
            sender: client_channels_send,
            receiver: client_channels_recv,
            network_info: NetworkInfo::default(),
        };

        (host_client, host_server)
    }
}

impl Client for HostClient {
    fn id(&self) -> ClientId {
        self.id
    }

    fn send_message(&mut self, channel_id: u8, message: Box<[u8]>) {
        if let Some(sender) = self.sender.get(&channel_id) {
            sender.try_send(message).unwrap();
        }
    }

    fn receive_all_messages_from_channel(&mut self, channel_id: u8) -> Vec<Box<[u8]>> {
        let mut messages = vec![];
        if let Some(receiver) = self.receiver.get(&channel_id) {
            while let Ok(message) = receiver.try_recv() {
                messages.push(message);
            }
        }
        messages
    }

    fn network_info(&mut self) -> &NetworkInfo {
        &self.network_info
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        Ok(())
    }

    fn process_events(&mut self, _current_time: Instant) -> Result<(), RenetError> {
        Ok(())
    }
}
