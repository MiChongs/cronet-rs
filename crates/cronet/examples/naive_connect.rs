//! Opens a SagerNet Cronet bidirectional CONNECT tunnel.

use std::io::{Read, Write};

use cronet::{Engine, EngineParams, NaiveConnectOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut params = EngineParams::new();
    params
        .enable_http2(true)
        .enable_quic(false)
        .http2_windows(128 * 1024 * 1024, 64 * 1024 * 1024)?
        .socket_pool_limits(2048, 2048, 2040)?;

    let engine = Engine::start(&params)?;
    let mut options = NaiveConnectOptions::new("https://proxy.example", "target.example:443");
    options.proxy_authorization = Some("Basic BASE64_USER_PASSWORD".into());
    let mut connection = engine.dial_naive(options)?;
    println!("negotiated {}", connection.handshake()?);

    connection.write_all(b"hello")?;
    let mut response = [0_u8; 1024];
    let count = connection.read(&mut response)?;
    println!("received {count} bytes");
    Ok(())
}
