use std::{
    io::{self, Read, Write},
    time::{Duration, Instant},
};

use crate::{BidirectionalConnection, Engine, Header};

const PADDING_CHUNKS: usize = 8;
const MAX_CHUNK: usize = u16::MAX as usize;

/// Parameters for an HTTP/2 or HTTP/3 Naive CONNECT tunnel.
#[derive(Clone, Debug)]
pub struct NaiveConnectOptions {
    /// HTTPS URL of the frontend/proxy server.
    pub proxy_url: String,
    /// Destination authority placed in the SagerNet extension header.
    pub destination: String,
    /// `Proxy-Authorization` value, commonly `Basic ...`.
    pub proxy_authorization: Option<String>,
    /// Additional request headers.
    pub extra_headers: Vec<Header>,
    /// Requests the SagerNet Cronet QUIC path.
    pub force_quic: bool,
    /// Optional network-isolation key used to create independent connection pools.
    pub network_isolation_key: Option<String>,
    /// Native stream priority.
    pub priority: i32,
    /// Internal asynchronous read size.
    pub read_buffer_size: usize,
}

impl NaiveConnectOptions {
    /// Creates options for one destination through one proxy URL.
    pub fn new(proxy_url: impl Into<String>, destination: impl Into<String>) -> Self {
        Self {
            proxy_url: proxy_url.into(),
            destination: destination.into(),
            proxy_authorization: None,
            extra_headers: Vec::new(),
            force_quic: false,
            network_isolation_key: None,
            priority: 0,
            read_buffer_size: 32 * 1024,
        }
    }
}

/// Naive padding-protocol connection over a Cronet bidirectional stream.
pub struct NaiveConnection<'engine> {
    inner: BidirectionalConnection<'engine>,
    read_padding: usize,
    write_padding: usize,
    read_remaining: usize,
    padding_remaining: usize,
}

impl Engine {
    /// Starts a Naive CONNECT tunnel without waiting for response headers.
    ///
    /// Call [`NaiveConnection::handshake`] before treating the connection as
    /// established. Keeping startup separate permits protocol Fast Open.
    pub fn dial_naive(&self, options: NaiveConnectOptions) -> io::Result<NaiveConnection<'_>> {
        let connection = self.create_bidirectional_connection(options.read_buffer_size);
        let mut headers = vec![
            Header {
                name: "-connect-authority".into(),
                value: options.destination,
            },
            Header {
                name: "Padding".into(),
                value: padding_header(),
            },
        ];
        if let Some(authorization) = options.proxy_authorization {
            headers.push(Header {
                name: "proxy-authorization".into(),
                value: authorization,
            });
        }
        if options.force_quic {
            headers.push(Header {
                name: "-force-quic".into(),
                value: "true".into(),
            });
        }
        if let Some(key) = options.network_isolation_key {
            headers.push(Header {
                name: "-network-isolation-key".into(),
                value: key,
            });
        }
        headers.extend(options.extra_headers);
        connection.start(
            "CONNECT",
            &options.proxy_url,
            &headers,
            options.priority,
            false,
        )?;
        Ok(NaiveConnection {
            inner: connection,
            read_padding: 0,
            write_padding: 0,
            read_remaining: 0,
            padding_remaining: 0,
        })
    }
}

impl NaiveConnection<'_> {
    /// Waits for CONNECT response headers and validates status 200.
    pub fn handshake(&self) -> io::Result<String> {
        let (headers, protocol) = self.inner.wait_for_headers()?;
        let status = headers
            .iter()
            .find(|header| header.name == ":status")
            .map(|header| header.value.as_str());
        if status != Some("200") {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionRefused,
                format!(
                    "Naive CONNECT returned status {}",
                    status.unwrap_or("missing")
                ),
            ));
        }
        Ok(protocol)
    }

    /// Cancels the underlying stream.
    pub fn cancel(&self) {
        self.inner.cancel();
    }

    /// Returns the unpadded stream adapter.
    pub fn inner(&self) -> &BidirectionalConnection<'_> {
        &self.inner
    }

    /// Sets an absolute deadline for reads, writes and the CONNECT handshake.
    pub fn set_deadline(&self, deadline: Option<Instant>) {
        self.inner.set_deadline(deadline);
    }

    /// Sets an absolute deadline for reads and the CONNECT handshake.
    pub fn set_read_deadline(&self, deadline: Option<Instant>) {
        self.inner.set_read_deadline(deadline);
    }

    /// Sets an absolute deadline for writes.
    pub fn set_write_deadline(&self, deadline: Option<Instant>) {
        self.inner.set_write_deadline(deadline);
    }

    /// Sets a relative read timeout. `None` disables it.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.set_read_timeout(timeout)
    }

    /// Sets a relative write timeout. `None` disables it.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.inner.set_write_timeout(timeout)
    }

    fn read_exact_inner(&self, mut output: &mut [u8]) -> io::Result<()> {
        while !output.is_empty() {
            let count = self.inner.read_shared(output)?;
            if count == 0 {
                return Err(io::Error::from(io::ErrorKind::UnexpectedEof));
            }
            output = &mut output[count..];
        }
        Ok(())
    }

    fn skip_inner(&self, mut count: usize) -> io::Result<()> {
        let mut scratch = [0_u8; 256];
        while count != 0 {
            let wanted = count.min(scratch.len());
            self.read_exact_inner(&mut scratch[..wanted])?;
            count -= wanted;
        }
        Ok(())
    }
}

impl Read for NaiveConnection<'_> {
    fn read(&mut self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        if self.read_remaining != 0 {
            let wanted = output.len().min(self.read_remaining);
            let count = self.inner.read_shared(&mut output[..wanted])?;
            self.read_remaining -= count;
            return Ok(count);
        }
        if self.padding_remaining != 0 {
            self.skip_inner(self.padding_remaining)?;
            self.padding_remaining = 0;
        }
        if self.read_padding >= PADDING_CHUNKS {
            return self.inner.read_shared(output);
        }

        let mut header = [0_u8; 3];
        self.read_exact_inner(&mut header)?;
        let payload = usize::from(u16::from_be_bytes([header[0], header[1]]));
        self.padding_remaining = usize::from(header[2]);
        self.read_padding += 1;
        if payload == 0 {
            self.skip_inner(self.padding_remaining)?;
            self.padding_remaining = 0;
            return self.read(output);
        }
        let wanted = output.len().min(payload);
        let count = self.inner.read_shared(&mut output[..wanted])?;
        self.read_remaining = payload.saturating_sub(count);
        Ok(count)
    }
}

impl Write for NaiveConnection<'_> {
    fn write(&mut self, input: &[u8]) -> io::Result<usize> {
        if input.is_empty() {
            return Ok(0);
        }
        let chunk_len = input.len().min(MAX_CHUNK);
        let chunk = &input[..chunk_len];
        if self.write_padding >= PADDING_CHUNKS {
            return self.inner.write_shared(chunk, false);
        }
        let padding = fastrand::usize(..=255);
        let mut frame = Vec::with_capacity(3 + chunk.len() + padding);
        frame.extend_from_slice(&(chunk.len() as u16).to_be_bytes());
        frame.push(padding as u8);
        frame.extend_from_slice(chunk);
        frame.resize(frame.len() + padding, 0);
        self.inner.write_shared(&frame, false)?;
        self.write_padding += 1;
        Ok(chunk.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush_shared();
        Ok(())
    }
}

fn padding_header() -> String {
    const ALPHABET: &[u8; 16] = b"!#$()+<>?@[]^`{}";
    let length = fastrand::usize(30..62);
    let mut bytes = vec![b'~'; length];
    for byte in bytes.iter_mut().take(16) {
        *byte = ALPHABET[fastrand::usize(..ALPHABET.len())];
    }
    // Every selected byte is ASCII.
    String::from_utf8(bytes).expect("padding alphabet is ASCII")
}
