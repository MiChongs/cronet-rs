//! Verifies native allocation, HTTPS requests and deterministic shutdown.

use std::{
    io,
    net::{IpAddr, ToSocketAddrs},
    thread,
    time::Duration,
};

use cronet::{
    Buffer, Client, Engine, EngineParams, NaiveClient, NaiveClientOptions, NetworkHooks,
    RequestOptions,
};
use hickory_proto::{
    op::Message,
    rr::{
        RData, Record, RecordType,
        rdata::{A, AAAA},
    },
};

fn system_dns(query: &[u8]) -> io::Result<Vec<u8>> {
    let request = Message::from_vec(query).map_err(io::Error::other)?;
    let mut response = request.clone().into_response();
    response.answers.clear();
    response.authorities.clear();
    response.additionals.clear();
    let Some(question) = request.queries.first() else {
        return response.to_vec().map_err(io::Error::other);
    };
    let host = question.name().to_utf8().trim_end_matches('.').to_owned();
    for address in (host.as_str(), 0).to_socket_addrs()? {
        let data = match (question.query_type(), address.ip()) {
            (RecordType::A, IpAddr::V4(address)) => Some(RData::A(A(address))),
            (RecordType::AAAA, IpAddr::V6(address)) => Some(RData::AAAA(AAAA(address))),
            _ => None,
        };
        if let Some(data) = data {
            response.add_answer(Record::from_rdata(question.name().clone(), 60, data));
        }
    }
    response.to_vec().map_err(io::Error::other)
}

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

    let naive = NaiveClient::start(
        NaiveClientOptions::new("https://www.example.com"),
        NetworkHooks::new().dns_resolver(system_dns),
    )?;
    let connection = naive.dial_early("target.example:443")?;
    connection.set_read_timeout(Some(Duration::from_secs(10)))?;
    let error = connection
        .handshake()
        .expect_err("example.com is not a NaiveProxy server");
    println!("bidirectional CONNECT rejected as expected: {error}");
    drop(connection);
    naive.shutdown();
    println!("naive client dropped");

    thread::sleep(Duration::from_millis(250));
    Ok(())
}
