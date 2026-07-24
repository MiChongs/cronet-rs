//! Performs a blocking GET and prints the response.

use cronet::{Client, Engine, EngineParams, RequestOptions};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let engine = Engine::start(&EngineParams::new())?;
    let client = Client::new(engine)?;
    let response = client.execute(RequestOptions::get("https://www.example.com"))?;
    println!("HTTP {}", response.info.status_code);
    println!("{}", String::from_utf8_lossy(&response.body));
    Ok(())
}
