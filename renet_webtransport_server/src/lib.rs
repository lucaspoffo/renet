use anyhow::Error;
use bytes::Bytes;
use h3::{error::ErrorLevel, ext::Protocol, server::Connection};
use h3_quinn::Connection as H3QuinnConnection;
use h3_webtransport::server::WebTransportSession;
use http::Method;
use log::{error, trace};
use renet::{ClientId, RenetServer};
use rustls::{Certificate, PrivateKey};
use std::{
    collections::{HashMap, HashSet},
    net::SocketAddr,
    path::PathBuf,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
    vec,
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
    /// Maximum number of active clients
    pub max_clients: usize,
}

struct WebTransportServerClient {
    session: Arc<WebTransportSession<H3QuinnConnection, bytes::Bytes>>,
    reader_reciever: mpsc::Receiver<Bytes>,
    reader_thread: tokio::task::JoinHandle<()>,
}

#[cfg_attr(feature = "bevy", derive(bevy_ecs::system::Resource))]
pub struct WebTransportServer {
    endpoint: quinn::Endpoint,
    clients: HashMap<ClientId, WebTransportServerClient>,
    lost_clients: HashSet<ClientId>,
    client_iterator: u64,
    connection_receiver: mpsc::Receiver<WebTransportSession<H3QuinnConnection, Bytes>>,
    connection_abort_handle: AbortHandle,
    current_clients: Arc<AtomicUsize>,
}

impl WebTransportServer {
    pub fn new(config: WebTransportConfig) -> Result<Self, Error> {
        let addr = config.listen;
        let max_clients = config.max_clients;
        let server_config = Self::create_server_config(config)?;
        let endpoint = quinn::Endpoint::server(server_config, addr)?;
        let (sender, receiver) = mpsc::channel::<WebTransportSession<H3QuinnConnection, Bytes>>(max_clients);
        let current_clients = Arc::new(AtomicUsize::new(0));
        let abort_handle = tokio::spawn(Self::accept_connection(
            sender,
            endpoint.clone(),
            Arc::clone(&current_clients),
            max_clients,
        ))
        .abort_handle();

        Ok(Self {
            endpoint,
            clients: HashMap::new(),
            client_iterator: 0,
            lost_clients: HashSet::new(),
            connection_receiver: receiver,
            connection_abort_handle: abort_handle,
            current_clients,
        })
    }

    pub fn update(&mut self, renet_server: &mut RenetServer) {
        let mut clients_added = 0;

        if let Ok(session) = self.connection_receiver.try_recv() {
            let shared_session = Arc::new(session);
            renet_server.add_connection(ClientId::from_raw(self.client_iterator));
            let (sender, reciever) = mpsc::channel::<Bytes>(256);
            let thread = Self::reading_thread(Arc::clone(&shared_session), sender);
            self.clients.insert(
                ClientId::from_raw(self.client_iterator),
                WebTransportServerClient {
                    session: shared_session,
                    reader_reciever: reciever,
                    reader_thread: thread,
                },
            );
            self.client_iterator += 1;
            clients_added += 1;
        }

        self.current_clients.fetch_add(clients_added, Ordering::Release);

        // recieve packets
        for (client_id, client_data) in self.clients.iter_mut() {
            let reader = &mut client_data.reader_reciever;
            while let Ok(packet) = reader.try_recv() {
                if let Err(e) = renet_server.process_packet_from(&packet, *client_id) {
                    error!("Error while processing payload for {}: {}", client_id, e);
                }
            }
        }

        for (client_id, client_data) in self.clients.iter() {
            if client_data.reader_thread.is_finished() {
                self.lost_clients.insert(*client_id);
            }
        }

        // remove lost clients
        self.current_clients.fetch_sub(self.lost_clients.len(), Ordering::Release);

        for client_id in self.lost_clients.drain() {
            self.clients.remove(&client_id);
            renet_server.remove_connection(client_id);
        }
    }

    pub fn send_packets(&mut self, renet_server: &mut RenetServer) {
        for (client_id, client_data) in self.clients.iter() {
            if let Ok(packets) = renet_server.get_packets_to_send(*client_id) {
                for packet in packets {
                    let data = Bytes::copy_from_slice(&packet);
                    if let Err(err) = client_data.session.send_datagram(data) {
                        match err.get_error_level() {
                            ErrorLevel::ConnectionError => {
                                self.lost_clients.insert(*client_id);
                                break;
                            }
                            ErrorLevel::StreamError => error!("Stream error: {err}"),
                        }
                    }
                }
            }
        }
    }

    async fn accept_connection(
        sender: mpsc::Sender<WebTransportSession<H3QuinnConnection, Bytes>>,
        endpoint: quinn::Endpoint,
        current_clients: Arc<AtomicUsize>,
        max_clients: usize,
    ) {
        while let Some(new_conn) = endpoint.accept().await {
            let sender = sender.clone();
            let current_clients = current_clients.clone();
            tokio::spawn(async move {
                match new_conn.await {
                    Ok(conn) => {
                        let is_full = {
                            let current_clients = current_clients.load(Ordering::Relaxed);
                            current_clients >= max_clients
                        };
                        if is_full {
                            conn.close(0u32.into(), b"Server full");
                            return;
                        }
                        if let Ok(h3_conn) = h3::server::builder()
                            .enable_webtransport(true)
                            .enable_connect(true)
                            .enable_datagram(true)
                            .max_webtransport_sessions(1)
                            .send_grease(true)
                            .build(H3QuinnConnection::new(conn))
                            .await
                        {
                            match Self::handle_connection(h3_conn).await {
                                Ok(web_transport_option) => {
                                    if let Some(session) = web_transport_option {
                                        if let Err(e) = sender.try_send(session) {
                                            error!("Failed to send session to main thread: {}", e);
                                        }
                                    }
                                }
                                Err(err) => {
                                    error!("Failed to handle connection: {err:?}");
                                }
                            }
                        }
                    }
                    Err(err) => {
                        error!("accepting connection failed: {:?}", err);
                    }
                }
            });
        }
    }

    pub fn disconnect(&mut self) {
        self.endpoint.close(0u32.into(), b"Server shutdown");
        self.connection_abort_handle.abort();
    }

    fn create_server_config(config: WebTransportConfig) -> Result<quinn::ServerConfig, Error> {
        let cert = Certificate(std::fs::read(config.cert)?);
        let key = PrivateKey(std::fs::read(config.key)?);

        let mut tls_config = rustls::ServerConfig::builder()
            .with_safe_default_cipher_suites()
            .with_safe_default_kx_groups()
            .with_protocol_versions(&[&rustls::version::TLS13])
            .unwrap()
            .with_no_client_auth()
            .with_single_cert(vec![cert], key)?;

        tls_config.max_early_data_size = u32::MAX;
        // We set the ALPN protocols to h3 as first, so that the browser will use the newest HTTP/3 draft and as fallback we use older versions of HTTP/3 draft
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
        mut conn: Connection<H3QuinnConnection, Bytes>,
    ) -> Result<Option<WebTransportSession<H3QuinnConnection, Bytes>>, h3::Error> {
        match conn.accept().await {
            Ok(Some((req, stream))) => {
                let ext = req.extensions();
                match req.method() {
                    &Method::CONNECT if ext.get::<Protocol>() == Some(&Protocol::WEB_TRANSPORT) => {
                        let session: WebTransportSession<H3QuinnConnection, Bytes> = WebTransportSession::accept(req, stream, conn).await?;
                        Ok(Some(session))
                    }
                    _ => Ok(None),
                }
            }

            // indicating no more streams to be received
            Ok(None) => Ok(None),

            Err(err) => Err(err),
        }
    }

    fn reading_thread(
        read_datagram: Arc<WebTransportSession<H3QuinnConnection, bytes::Bytes>>,
        sender: mpsc::Sender<Bytes>,
    ) -> tokio::task::JoinHandle<()> {
        tokio::spawn(async move {
            loop {
                let datagram = read_datagram.accept_datagram();
                let result = datagram.await;
                if result.is_err() {
                    break;
                }
                match result.unwrap() {
                    Some((_, datagram_bytes)) => match sender.try_send(datagram_bytes) {
                        Ok(_) => {}
                        Err(err) => {
                            if let mpsc::error::TrySendError::Closed(_) = err {
                                break;
                            }
                            trace!("The reading data could not be sent because the channel is currently full and sending would require blocking.");
                        }
                    },
                    None => break,
                }
            }
        })
    }
}
