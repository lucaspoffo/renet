use bytes::Bytes;
use h3::{error::ErrorLevel, ext::Protocol, server::Connection};
use h3_webtransport::server::WebTransportSession;
use http::Method;
use log::{error, info, debug};
use renet::{ClientId, RenetServer};
use rustls::{Certificate, PrivateKey};
use std::{
    collections::HashMap,
    io::Error,
    net::SocketAddr,
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::{sync::mpsc, task::AbortHandle};

pub struct WebTransportConfig {
    /// Path to the certificate file, must be DER encoded
    pub cert: PathBuf,
    /// Path to the certificate-key file, must be DER encoded
    pub key: PathBuf,
    /// Socket address to listen on
    ///
    ///  
    /// Info:
    /// Platform defaults for dual-stack sockets vary. For example, any socket bound to a wildcard IPv6 address on Windows will not by default be able to communicate with IPv4 addresses. Portable applications should bind an address that matches the family they wish to communicate within.
    pub listen: SocketAddr,
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct WebTransportServer {
    endpoint: quinn::Endpoint,
    clients: HashMap<ClientId, Arc<WebTransportSession<h3_quinn::Connection, bytes::Bytes>>>,
    lost_clients: Vec<ClientId>,
    client_iterator: u64,
    connection_receiver: mpsc::Receiver<WebTransportSession<h3_quinn::Connection, Bytes>>,
    connection_abort_handle: AbortHandle,
    reader_recievers: HashMap<ClientId, mpsc::Receiver<Bytes>>,
    reader_threads: HashMap<ClientId, tokio::task::JoinHandle<()>>,
}

impl WebTransportServer {
    pub fn new(config: WebTransportConfig) -> Result<Self, Error> {
        let addr = config.listen;
        let server_config = Self::create_server_config(config)?;
        let endpoint = quinn::Endpoint::server(server_config, addr)?;
        let (sender, receiver) = mpsc::channel::<WebTransportSession<h3_quinn::Connection, Bytes>>(32);
        let handle = tokio::spawn(Self::accept_connection(sender, endpoint.clone()));
        let abort_handle = handle.abort_handle();

        Ok(Self {
            endpoint,
            clients: HashMap::new(),
            client_iterator: 0,
            lost_clients: Vec::new(),
            connection_receiver: receiver,
            connection_abort_handle: abort_handle,
            reader_recievers: HashMap::new(),
            reader_threads: HashMap::new(),
        })
    }

    pub fn update(&mut self, renet_server: &mut RenetServer) {
        if let Ok(session) = self.connection_receiver.try_recv() {
            debug!("Recieving new connection");
            let shared_session = Arc::new(session);
            renet_server.add_connection(ClientId::from_raw(self.client_iterator));
            let (sender, reciever) = mpsc::channel::<Bytes>(256);
            self.reader_recievers.insert(ClientId::from_raw(self.client_iterator), reciever);
            let thread = Self::reading_thread(ClientId::from_raw(self.client_iterator), Arc::clone(&shared_session), sender);
            self.clients.insert(ClientId::from_raw(self.client_iterator), shared_session);
            self.reader_threads.insert(ClientId::from_raw(self.client_iterator), thread);
            self.client_iterator += 1;
            debug!("Finished new connection handling");
        }
        // recieve packets
        for (client_id, _) in self.clients.iter_mut() {
            let reader = self.reader_recievers.get_mut(client_id).unwrap();
            while let Ok(packet) = reader.try_recv() {
                debug!("Recieving packet from client {}", client_id);
                let result= renet_server.process_packet_from(&packet, *client_id);
                if result.is_err() {
                    info!("Client {} not found {}", client_id, result.err().unwrap());
                    self.lost_clients.push(*client_id);
                    break;
                }
            }
        }
        for (client_id, thread) in self.reader_threads.iter()  {
            if thread.is_finished() {
                debug!("Client {} thread died", client_id);
                self.lost_clients.push(*client_id);
            }
            
        }
        // remove lost clients
        for client_id in self.lost_clients.iter() {
            self.clients.remove(client_id);
            renet_server.remove_connection(*client_id);
            self.reader_recievers.remove(client_id);
            self.reader_threads.remove(client_id);
        }
    }

    pub fn send_packets(&mut self, renet_server: &mut RenetServer) {
        for (client_id, session) in self.clients.iter() {
            if let Ok(packets) = renet_server.get_packets_to_send(*client_id) {
                for packet in packets {
                    debug!("Sending packet to client {}", client_id);
                    let data = Bytes::copy_from_slice(&packet);
                    if let Err(err) = session.send_datagram(data) {
                        match err.get_error_level() {
                            ErrorLevel::ConnectionError => {
                                debug!("connection error for {}", client_id);
                                self.lost_clients.push(*client_id);
                                break;
                            }
                            ErrorLevel::StreamError => error!("Stream error: {err}"),
                        }
                    }
                }
            }
        }
    }

    async fn accept_connection(sender: mpsc::Sender<WebTransportSession<h3_quinn::Connection, Bytes>>, endpoint: quinn::Endpoint) {
        while let Some(new_conn) = endpoint.accept().await {
            info!("New connection being attempted");
            match new_conn.await {
                Ok(conn) => {
                    info!("new http3 established");
                    let h3_conn = h3::server::builder()
                        .enable_webtransport(true)
                        .enable_connect(true)
                        .enable_datagram(true)
                        .max_webtransport_sessions(1)
                        .send_grease(true)
                        .build(h3_quinn::Connection::new(conn))
                        .await
                        .unwrap();

                    match Self::handle_connection(h3_conn).await {
                        Ok(web_transport_option) => {
                            if let Some(session) = web_transport_option {
                                info!("WebTransport session established");
                                let result = sender.send(session).await; // TODO handle the error case somehow
                            }
                        }
                        Err(err) => {
                            error!("Failed to handle connection: {err:?}");
                        }
                    }
                }
                Err(err) => {
                    error!("accepting connection failed: {:?}", err);
                }
            }
        }
    }

    pub fn disconnect(&mut self) {
        self.endpoint.close(0u32.into(), b"Server shutdown");
        self.connection_abort_handle.abort();
    }

    fn create_server_config(config: WebTransportConfig) -> Result<quinn::ServerConfig, std::io::Error> {
        let cert = Certificate(std::fs::read(config.cert)?);
        let key = PrivateKey(std::fs::read(config.key)?);

        let mut tls_config = rustls::ServerConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)
            .unwrap(); // TODO handle Errorcase

        tls_config.max_early_data_size = u32::MAX;
        let alpn: Vec<Vec<u8>> = vec![
            b"h3".to_vec(),
            b"h3-32".to_vec(),
            b"h3-31".to_vec(),
            b"h3-30".to_vec(),
            b"h3-29".to_vec(),
        ];
        tls_config.alpn_protocols = alpn;

        let mut server_config: quinn::ServerConfig = quinn::ServerConfig::with_crypto(Arc::new(tls_config));
        let mut transport_config = quinn::TransportConfig::default();
        transport_config.keep_alive_interval(Some(Duration::from_secs(2)));
        server_config.transport = Arc::new(transport_config);
        Ok(server_config)
    }

    async fn handle_connection(
        mut conn: Connection<h3_quinn::Connection, Bytes>,
    ) -> Result<Option<WebTransportSession<h3_quinn::Connection, Bytes>>, h3::Error> {
        match conn.accept().await {
            Ok(Some((req, stream))) => {
                info!("new request: {:#?}", req);
                let ext = req.extensions();
                match req.method() {
                    &Method::CONNECT if ext.get::<Protocol>() == Some(&Protocol::WEB_TRANSPORT) => {
                        info!("Peer wants to initiate a webtransport session");

                        info!("Handing over connection to WebTransport");
                        let session: WebTransportSession<h3_quinn::Connection, Bytes> =
                            WebTransportSession::accept(req, stream, conn).await?;
                        info!("Established webtransport session");
                        // 4. Get datagrams and wait for client requests here.
                        // h3_conn needs to handover the datagrams to the webtransport session.
                        //let datagram: h3_webtransport::server::ReadDatagram<'_, h3_quinn::Connection, Bytes> = session.accept_datagram();
                        Ok(Some(session))
                    }
                    _ => {
                        info!("Peer wants to initiate a http3 session");
                        Ok(None)
                    }
                }
            }

            // indicating no more streams to be received
            Ok(None) => {
                info!("No more streams to be received");
                Ok(None)
            }

            Err(err) => {
                error!("Error on accept {}", err);
                match err.get_error_level() {
                    ErrorLevel::ConnectionError => error!("Connection error: {err}"),
                    ErrorLevel::StreamError => error!("Stream error: {err}"),
                }
                Err(err)
            }
        }
    }

    fn reading_thread(
        client_id: ClientId,
        read_datagram: Arc<WebTransportSession<h3_quinn::Connection, bytes::Bytes>>,
        sender: mpsc::Sender<Bytes>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let datagram = read_datagram.accept_datagram();
                let result = datagram.await;
                info!("Recieving packets from client {}", client_id);
                if result.is_err() {
                    //self.lost_clients.push(*client_id);
                    info!("Client {} disconnected with error {}", client_id, result.err().unwrap());
                    break;
                }
                match result.unwrap() {
                    Some((_, datagram_bytes)) => match sender.try_send(datagram_bytes) {
                        Ok(_) => {
                            info!("Sent packet from client {}", client_id)
                        }
                        Err(err) => {
                            error!("Failed to send packet from client {}: {}", client_id, err);
                        }
                    },
                    None => break,
                }
                info!("Finished recieving packets from client {}", client_id);
            }
        })
    }
}
