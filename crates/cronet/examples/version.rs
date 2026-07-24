//! Prints version information from a locally installed Cronet SDK.

use cronet::{Engine, EngineParams};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut params = EngineParams::new();
    params
        .enable_quic(true)
        .enable_http2(true)
        .enable_brotli(true);
    let engine = Engine::start(&params)?;
    println!("Cronet {}", engine.version());
    println!("User-Agent: {}", engine.default_user_agent());
    Ok(())
}
