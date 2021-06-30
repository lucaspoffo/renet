use crate::channel::ChannelConfig;
use crate::error::RenetError;
use crate::packet::{ConnectionHeader, HeaderParser};
use crate::protocol::AuthenticationProtocol;
use crate::remote_connection::{ClientId, ConnectionConfig};
use crate::Timer;

use log::{debug, error, info};

use std::collections::HashMap;
use std::io;
use std::net::{SocketAddr, UdpSocket};

use crate::client::RemoteClientConnected;

pub struct RequestConnection<P> {
    socket: UdpSocket,
    server_addr: SocketAddr,
    id: ClientId,
    protocol: P,
    connection_config: ConnectionConfig,
    channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
    buffer: Box<[u8]>,
    timeout_timer: Timer,
}

impl<P: AuthenticationProtocol> RequestConnection<P> {
    pub fn new(
        id: ClientId,
        socket: UdpSocket,
        server_addr: SocketAddr,
        protocol: P,
        connection_config: ConnectionConfig,
        channels_config: HashMap<u8, Box<dyn ChannelConfig>>,
    ) -> Result<Self, RenetError> {
        socket.set_nonblocking(true)?;
        let buffer = vec![0; connection_config.max_packet_size].into_boxed_slice();
        let timeout_timer = Timer::new(connection_config.timeout_duration);

        let channels_config: HashMap<u8, Box<dyn ChannelConfig>> = channels_config
            .into_iter()
            .map(|(k, v)| (k.into(), v))
            .collect();

        Ok(Self {
            id,
            socket,
            server_addr,
            protocol,
            channels_config,
            connection_config,
            timeout_timer,
            buffer,
        })
    }

    pub(crate) fn process_payload(&mut self, payload: &[u8]) -> Result<(), RenetError> {
        self.timeout_timer.reset();
        let connection_header = ConnectionHeader::parse(&payload[..1])?;
        if let Some(e) = connection_header.error {
            return Err(RenetError::ConnectionError(e));
        }
        let payload = &payload[1..];
        self.protocol.read_payload(payload)?;
        Ok(())
    }

    pub fn update(&mut self) -> Result<Option<RemoteClientConnected<P::Service>>, RenetError> {
        self.process_events()?;

        if self.timeout_timer.is_finished() {
            error!("Connection with the server timed out");
            return Err(RenetError::ConnectionTimedOut);
        }

        if self.protocol.is_authenticated() {
            let security_service = self.protocol.build_security_interface();
            return Ok(Some(RemoteClientConnected::new(
                self.id,
                self.socket.try_clone()?,
                self.server_addr,
                self.channels_config.clone(),
                security_service,
                self.connection_config.clone(),
            )));
        }

        match self.protocol.create_payload() {
            Ok(Some(payload)) => {
                info!("Sending protocol payload to server: {:?}", payload);
                self.socket.send_to(&payload, self.server_addr)?;
            }
            Ok(None) => {}
            Err(e) => error!("Failed to create protocol payload: {:?}", e),
        }

        Ok(None)
    }

    pub(crate) fn process_events(&mut self) -> Result<(), RenetError> {
        loop {
            let payload = match self.socket.recv_from(&mut self.buffer) {
                Ok((len, addr)) => {
                    if addr == self.server_addr {
                        self.buffer[..len].to_vec().into_boxed_slice()
                    } else {
                        debug!("Discarded packet from unknown server {:?}", addr);
                        continue;
                    }
                }
                Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(RenetError::IOError(e)),
            };

            self.process_payload(&payload)?;
        }
    }
}
