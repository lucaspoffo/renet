use std::sync::{Arc, RwLock};

use crate::client::Client;
use crate::error::{DisconnectionReason, RenetError};
use crate::packet::Payload;
use crate::remote_connection::NetworkInfo;
use crate::ClientId;

use crossbeam_channel::{unbounded, Receiver, Sender, TryRecvError};
use log::error;

pub struct LocalClient<C> {
    pub id: C,
    sender: Sender<Payload>,
    receiver: Receiver<Payload>,
    disconnect_reason: Arc<RwLock<Option<DisconnectionReason>>>,
}

impl<C> LocalClient<C> {
    pub fn is_connected(&self) -> bool {
        self.disconnect_reason.read().unwrap().is_none()
    }

    pub fn disconnect(&mut self) {
        let mut disconnect_reason = self.disconnect_reason.write().unwrap();
        *disconnect_reason = Some(DisconnectionReason::DisconnectedByServer);
    }

    pub fn send_message(&self, message: Payload) {
        if let Err(e) = self.sender.send(message) {
            error!("Error sending message to local connection: {}", e);

            let mut disconnect_reason = self.disconnect_reason.write().unwrap();
            *disconnect_reason = Some(DisconnectionReason::CrossbeamChannelError);
        }
    }

    pub fn receive_message(&self) -> Option<Payload> {
        match self.receiver.try_recv() {
            Ok(payload) => Some(payload),
            Err(TryRecvError::Empty) => None,
            Err(e) => {
                error!("Error receiving message in local connection: {}", e);

                let mut disconnect_reason = self.disconnect_reason.write().unwrap();
                *disconnect_reason = Some(DisconnectionReason::CrossbeamChannelError);
                None
            }
        }
    }
}

pub struct LocalClientConnected<C> {
    id: C,
    sender: Sender<Payload>,
    receiver: Receiver<Payload>,
    network_info: NetworkInfo,
    disconnect_reason: Arc<RwLock<Option<DisconnectionReason>>>,
}

impl<C: ClientId> LocalClientConnected<C> {
    pub fn new(client_id: C) -> (LocalClientConnected<C>, LocalClient<C>) {
        let (channel_send, host_recv) = unbounded();
        let (host_send, channel_recv) = unbounded();

        let disconnect_reason = Arc::new(RwLock::new(None));

        let host_server = LocalClient {
            disconnect_reason: disconnect_reason.clone(),
            id: client_id,
            sender: host_send,
            receiver: host_recv,
        };

        let host_client = LocalClientConnected {
            disconnect_reason,
            id: client_id,
            sender: channel_send,
            receiver: channel_recv,
            network_info: NetworkInfo::default(),
        };

        (host_client, host_server)
    }

    pub fn send_message(&mut self, message: Payload) {
        if let Err(e) = self.sender.try_send(message) {
            error!("Error sending message to local connection: {}", e);

            let mut disconnect_reason = self.disconnect_reason.write().unwrap();
            *disconnect_reason = Some(DisconnectionReason::CrossbeamChannelError);
        }
    }
}

impl<C: ClientId> Client<C> for LocalClientConnected<C> {
    fn id(&self) -> C {
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

    fn send_reliable_message(&mut self, _channel_id: u8, message: Payload) {
        self.send_message(message);
    }

    fn send_unreliable_message(&mut self, message: Payload) {
        self.send_message(message);
    }

    fn send_block_message(&mut self, message: Payload) {
        self.send_message(message);
    }

    fn receive_message(&mut self) -> Option<Payload> {
        match self.receiver.try_recv() {
            Ok(payload) => Some(payload),
            Err(TryRecvError::Empty) => None,
            Err(e) => {
                error!("Error receiving message in local connection: {}", e);

                let mut disconnect_reason = self.disconnect_reason.write().unwrap();
                *disconnect_reason = Some(DisconnectionReason::CrossbeamChannelError);
                None
            }
        }
    }

    fn network_info(&self) -> &NetworkInfo {
        &self.network_info
    }

    fn send_packets(&mut self) -> Result<(), RenetError> {
        Ok(())
    }

    fn update(&mut self) -> Result<(), RenetError> {
        Ok(())
    }
}
