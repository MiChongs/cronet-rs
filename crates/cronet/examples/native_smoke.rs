//! Verifies native allocation, HTTPS requests and deterministic shutdown.

use std::{thread, time::Duration};

use cronet::{Buffer, Client, Engine, EngineParams, RequestOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    drop(Buffer::new(16));
    println!("standalone buffer dropped");
    let engine = Engine::start(&EngineParams::new())?;
    println!("engine started");
    let client = Client::new(engine)?;
    let response = client.execute(RequestOptions::get("https://www.example.com"))?;
    println!("request completed: HTTP {}", response.info.status_code);
    drop(response);
    println!("response dropped");
    thread::sleep(Duration::from_millis(250));
    println!("dropping client");
    drop(client);
    println!("client dropped");
    thread::sleep(Duration::from_millis(250));
    Ok(())
}
