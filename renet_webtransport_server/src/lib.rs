use bytes::{BufMut, Bytes, BytesMut};
use h3::{error::ErrorLevel, ext::Protocol, server::Connection};
use h3_webtransport::server::WebTransportSession;
use http::Method;
use log::{debug, error, info};
use renet::{ClientId, RenetServer};
use rustls::{Certificate, PrivateKey};
use std::{collections::HashMap, io::Error, net::SocketAddr, path::PathBuf, sync::Arc, time::Duration};
use tokio::task::JoinHandle;

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
    clients: HashMap<ClientId, WebTransportSession<h3_quinn::Connection, Bytes>>,
    lost_clients: Vec<ClientId>,
    client_iterator: u64,
}

impl WebTransportServer {
    pub fn new(config: WebTransportConfig) -> Result<Self, Error> {
        let addr = config.listen;
        let server_config = Self::create_server_config(config)?;
        let endpoint = quinn::Endpoint::server(server_config, addr)?;

        Ok(Self {
            endpoint,
            clients: HashMap::new(),
            client_iterator: 0,
            lost_clients: Vec::new(),
        })
    }

    pub async fn update(&mut self, renet_server: &mut RenetServer) {
        // new clients
        let mut new_clients = Vec::new();
        debug!("Waiting for new connections");
        while let Some(new_conn) = self.endpoint.accept().await {
            debug!("New connection being attempted");
            let join_handle: JoinHandle<Option<WebTransportSession<h3_quinn::Connection, Bytes>>> = tokio::spawn(async move {
                match new_conn.await {
                    Ok(conn) => {
                        debug!("new http3 established");
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
                                    return Some(session);
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
                None
            });
            new_clients.push(join_handle);
        }
        // join threads and get the sessions
        debug!("Joining threads");
        for new_client in new_clients {
            if let Some(session) = new_client.await.unwrap() {
                renet_server.add_connection(ClientId::from_raw(self.client_iterator));
                self.clients.insert(ClientId::from_raw(self.client_iterator), session);
                self.client_iterator += 1;
            }
        }
        debug!("Finished joining threads");
        // recieve packets
        for (client_id, session) in self.clients.iter_mut() {
            loop {
                let datagram: h3_webtransport::server::ReadDatagram<'_, h3_quinn::Connection, Bytes> = session.accept_datagram();
                if let Some((_, datagram_bytes)) = datagram.await.unwrap() {
                    let _ = renet_server.process_packet_from(&datagram_bytes, *client_id);
                } else {
                    break;
                }
            }
        }
    }

    pub fn send_packets(&mut self, renet_server: &mut RenetServer) {
        for (client_id, session) in self.clients.iter_mut() {
            if let Ok(packets) = renet_server.get_packets_to_send(*client_id) {
                for packet in packets {
                    let data = Bytes::copy_from_slice(&packet);
                    if let Err(err) = session.send_datagram(data) {
                        match err.get_error_level() {
                            ErrorLevel::ConnectionError => {
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

    pub fn disconnect(&mut self) {
        self.endpoint.close(0u32.into(), b"Server shutdown");
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
                debug!("new request: {:#?}", req);
                let ext = req.extensions();
                match req.method() {
                    &Method::CONNECT if ext.get::<Protocol>() == Some(&Protocol::WEB_TRANSPORT) => {
                        debug!("Peer wants to initiate a webtransport session");

                        debug!("Handing over connection to WebTransport");
                        let session: WebTransportSession<h3_quinn::Connection, Bytes> =
                            WebTransportSession::accept(req, stream, conn).await?;
                        debug!("Established webtransport session");
                        // 4. Get datagrams and wait for client requests here.
                        // h3_conn needs to handover the datagrams to the webtransport session.
                        let datagram: h3_webtransport::server::ReadDatagram<'_, h3_quinn::Connection, Bytes> = session.accept_datagram();
                        if let Some((x, datagram_bytes)) = datagram.await? {
                            debug!("Getting {datagram_bytes:?} from {x:?}");
                            // Put something before to make sure encoding and decoding works and don't just
                            // pass through
                            let mut resp = BytesMut::from(&b"Response: "[..]);
                            resp.put(datagram_bytes);

                            session.send_datagram(resp.freeze())?;
                            info!("Finished sending datagram");
                        }
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
}
