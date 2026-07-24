//! Runs a successful Naive CONNECT and padded echo over native Cronet H2.

use std::{
    future::poll_fn,
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
    thread,
    time::Duration,
};

use bytes::Bytes;
use cronet::{NaiveClient, NaiveClientOptions, NetworkHooks};
use h2::server::SendResponse;
use http::{Request, Response};
use rcgen::{
    BasicConstraints, CertificateParams, ExtendedKeyUsagePurpose, IsCa, KeyPair, KeyUsagePurpose,
};
use rustls::{
    ServerConfig,
    pki_types::{CertificateDer, PrivateKeyDer, PrivatePkcs8KeyDer},
};
use time::{Duration as TimeDuration, OffsetDateTime};
use tokio::{net::TcpListener as TokioTcpListener, runtime::Builder, sync::oneshot};
use tokio_rustls::TlsAcceptor;

struct LocalNaiveServer {
    port: u16,
    root_pem: String,
    shutdown: Option<oneshot::Sender<()>>,
    worker: Option<thread::JoinHandle<()>>,
}

impl LocalNaiveServer {
    fn start() -> Result<Self, Box<dyn std::error::Error>> {
        let ca_key = KeyPair::generate()?;
        let mut ca_params = CertificateParams::new(vec!["cronet-rs test CA".to_owned()])?;
        let now = OffsetDateTime::now_utc();
        ca_params.not_before = now - TimeDuration::days(1);
        ca_params.not_after = now + TimeDuration::days(30);
        ca_params.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
        ca_params.key_usages = vec![
            KeyUsagePurpose::KeyCertSign,
            KeyUsagePurpose::CrlSign,
            KeyUsagePurpose::DigitalSignature,
        ];
        let ca = ca_params.self_signed(&ca_key)?;

        let server_key = KeyPair::generate()?;
        let mut server_params = CertificateParams::new(vec!["localhost".to_owned()])?;
        server_params.not_before = now - TimeDuration::days(1);
        server_params.not_after = now + TimeDuration::days(30);
        server_params.extended_key_usages = vec![ExtendedKeyUsagePurpose::ServerAuth];
        let server_certificate = server_params.signed_by(&server_key, &ca, &ca_key)?;
        let root_pem = ca.pem();
        let certificate: CertificateDer<'static> = server_certificate.der().clone();
        let private_key =
            PrivateKeyDer::Pkcs8(PrivatePkcs8KeyDer::from(server_key.serialize_der()));
        let mut tls = ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(vec![certificate], private_key)?;
        tls.alpn_protocols = vec![b"h2".to_vec()];

        let listener = TcpListener::bind(("127.0.0.1", 0))?;
        let port = listener.local_addr()?.port();
        listener.set_nonblocking(true)?;
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let (started_tx, started_rx) = mpsc::sync_channel(1);
        let worker = thread::spawn(move || {
            let runtime = Builder::new_current_thread().enable_all().build().unwrap();
            runtime.block_on(async move {
                let listener = TokioTcpListener::from_std(listener).unwrap();
                let acceptor = TlsAcceptor::from(std::sync::Arc::new(tls));
                started_tx.send(()).unwrap();
                tokio::select! {
                    result = serve_one(listener, acceptor) => result.unwrap(),
                    _ = shutdown_rx => {}
                }
            });
        });
        started_rx.recv_timeout(Duration::from_secs(5))?;
        Ok(Self {
            port,
            root_pem,
            shutdown: Some(shutdown_tx),
            worker: Some(worker),
        })
    }
}

impl Drop for LocalNaiveServer {
    fn drop(&mut self) {
        if let Some(shutdown) = self.shutdown.take() {
            let _ = shutdown.send(());
        }
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

async fn serve_one(
    listener: TokioTcpListener,
    acceptor: TlsAcceptor,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let (socket, _) = listener.accept().await?;
    let socket = acceptor.accept(socket).await?;
    let mut connection = h2::server::handshake(socket).await?;
    while let Some(request) = connection.accept().await {
        let (request, respond) = request?;
        tokio::spawn(async move {
            let _ = serve_connect(request, respond).await;
        });
    }
    Ok(())
}

async fn serve_connect(
    request: Request<h2::RecvStream>,
    mut respond: SendResponse<Bytes>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if request.method() != http::Method::CONNECT {
        respond.send_response(Response::builder().status(405).body(())?, true)?;
        return Ok(());
    }
    let mut request_body = request.into_body();
    let mut response_body =
        respond.send_response(Response::builder().status(200).body(())?, false)?;
    while let Some(packet) = request_body.data().await {
        let packet = packet?;
        request_body.flow_control().release_capacity(packet.len())?;
        response_body.reserve_capacity(packet.len());
        while response_body.capacity() < packet.len() {
            match poll_fn(|context| response_body.poll_capacity(context)).await {
                Some(Ok(_)) => {}
                Some(Err(error)) => return Err(error.into()),
                None => return Ok(()),
            }
        }
        response_body.send_data(packet, false)?;
    }
    response_body.send_data(Bytes::new(), true)?;
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let server = LocalNaiveServer::start()?;
    let options = NaiveClientOptions::new(format!("https://localhost:{}", server.port));
    let hooks = NetworkHooks::new().trusted_root_certificates(server.root_pem.clone());
    let client = NaiveClient::start(options, hooks)?;
    let mut connection = client.dial_early("echo.invalid:443")?;
    connection.set_read_timeout(Some(Duration::from_secs(10)))?;
    connection.set_write_timeout(Some(Duration::from_secs(10)))?;
    connection.handshake()?;

    let payloads = [
        b"one".as_slice(),
        b"two-two".as_slice(),
        b"three-three-three".as_slice(),
        b"4".as_slice(),
        b"five".as_slice(),
        b"six".as_slice(),
        b"seven".as_slice(),
        b"eight".as_slice(),
        b"ninth chunk is deliberately unpadded".as_slice(),
    ];
    for payload in payloads {
        connection.write_all(payload)?;
        let mut echoed = vec![0; payload.len()];
        connection.read_exact(&mut echoed)?;
        assert_eq!(echoed, payload);
    }

    drop(connection);
    client.shutdown();
    println!("native H2 Naive CONNECT and padded echo succeeded");
    Ok(())
}
