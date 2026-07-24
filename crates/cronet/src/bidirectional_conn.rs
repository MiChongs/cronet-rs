use std::{
    io::{self, Read, Write},
    sync::{Arc, Condvar, Mutex},
};

use crate::{
    BidirectionalStream, BidirectionalStreamHandle, BidirectionalStreamHandler, Engine, Header,
};

#[derive(Default)]
struct Control {
    ready: bool,
    headers: Option<Vec<Header>>,
    negotiated_protocol: String,
    terminal: Option<io::Error>,
}

struct ReadState {
    buffer: Box<[u8]>,
    pending: bool,
    completed: Option<usize>,
    eof: bool,
}

#[derive(Default)]
struct WriteState {
    buffer: Vec<u8>,
    pending: bool,
}

struct ConnectionState {
    control: Mutex<Control>,
    control_event: Condvar,
    read: Mutex<ReadState>,
    read_event: Condvar,
    write: Mutex<WriteState>,
    write_event: Condvar,
}

/// Socket-like blocking adapter over a Cronet H2/H3 bidirectional stream.
///
/// One read and one write may be outstanding concurrently, matching Cronet's
/// stream contract. Internally owned buffers remain stable across native calls.
pub struct BidirectionalConnection<'engine> {
    stream: BidirectionalStream<'engine>,
    state: Arc<ConnectionState>,
}

impl Engine {
    /// Creates a socket-like bidirectional connection.
    pub fn create_bidirectional_connection(
        &self,
        read_buffer_size: usize,
    ) -> BidirectionalConnection<'_> {
        let state = Arc::new(ConnectionState {
            control: Mutex::new(Control::default()),
            control_event: Condvar::new(),
            read: Mutex::new(ReadState {
                buffer: vec![0; read_buffer_size.max(1)].into_boxed_slice(),
                pending: false,
                completed: None,
                eof: false,
            }),
            read_event: Condvar::new(),
            write: Mutex::new(WriteState::default()),
            write_event: Condvar::new(),
        });
        let stream = self.create_bidirectional_stream(ConnectionHandler {
            state: Arc::clone(&state),
        });
        BidirectionalConnection { stream, state }
    }
}

impl BidirectionalConnection<'_> {
    /// Starts a bidirectional request.
    pub fn start(
        &self,
        method: &str,
        url: &str,
        headers: &[Header],
        priority: i32,
        end_of_stream: bool,
    ) -> io::Result<()> {
        if self
            .stream
            .start(method, url, headers, priority, end_of_stream)
        {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Cronet rejected bidirectional stream parameters",
            ))
        }
    }

    /// Waits until response headers arrive.
    pub fn wait_for_headers(&self) -> io::Result<(Vec<Header>, String)> {
        let control = self
            .state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let control = self
            .state
            .control_event
            .wait_while(control, |state| {
                state.headers.is_none() && state.terminal.is_none()
            })
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(error) = control.terminal.as_ref() {
            return Err(clone_io_error(error));
        }
        Ok((
            control.headers.clone().unwrap_or_default(),
            control.negotiated_protocol.clone(),
        ))
    }

    /// Waits until Cronet reports that reads and writes may begin.
    pub fn wait_ready(&self) -> io::Result<()> {
        let control = self
            .state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let control = self
            .state
            .control_event
            .wait_while(control, |state| !state.ready && state.terminal.is_none())
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match control.terminal.as_ref() {
            Some(error) => Err(clone_io_error(error)),
            None => Ok(()),
        }
    }

    /// Reads response data using a separately synchronized read direction.
    pub fn read_shared(&self, output: &mut [u8]) -> io::Result<usize> {
        if output.is_empty() {
            return Ok(0);
        }
        self.wait_ready()?;
        let mut read = self
            .state
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if read.eof {
            return Ok(0);
        }
        read.completed = None;
        read.pending = true;
        // SAFETY: Box allocation is stable and read state prevents another
        // access until the callback clears `pending`.
        let result = unsafe { self.stream.read(&mut read.buffer) };
        if result != 0 {
            read.pending = false;
            return Err(io::Error::other(format!(
                "bidirectional_stream_read returned {result}"
            )));
        }
        let read = self
            .state
            .read_event
            .wait_while(read, |state| state.pending)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(error) = self.terminal_error() {
            return Err(error);
        }
        let count = read.completed.unwrap_or(0).min(read.buffer.len());
        let copied = count.min(output.len());
        output[..copied].copy_from_slice(&read.buffer[..copied]);
        Ok(copied)
    }

    /// Writes request data using a separately synchronized write direction.
    pub fn write_shared(&self, input: &[u8], end_of_stream: bool) -> io::Result<usize> {
        self.wait_ready()?;
        let mut write = self
            .state
            .write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        write.buffer.clear();
        write.buffer.extend_from_slice(input);
        write.pending = true;
        // SAFETY: Write buffer is not changed until completion callback.
        let result = unsafe { self.stream.write(&write.buffer, end_of_stream) };
        if result != 0 {
            write.pending = false;
            return Err(io::Error::other(format!(
                "bidirectional_stream_write returned {result}"
            )));
        }
        drop(
            self.state
                .write_event
                .wait_while(write, |state| state.pending)
                .unwrap_or_else(std::sync::PoisonError::into_inner),
        );
        if let Some(error) = self.terminal_error() {
            return Err(error);
        }
        Ok(input.len())
    }

    /// Flushes pending writes.
    pub fn flush_shared(&self) {
        self.stream.flush();
    }

    /// Cancels the connection.
    pub fn cancel(&self) {
        self.stream.cancel();
    }

    fn terminal_error(&self) -> Option<io::Error> {
        self.state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminal
            .as_ref()
            .map(clone_io_error)
    }
}

impl Read for BidirectionalConnection<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.read_shared(buf)
    }
}

impl Write for BidirectionalConnection<'_> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write_shared(buf, false)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush_shared();
        Ok(())
    }
}

struct ConnectionHandler {
    state: Arc<ConnectionState>,
}

impl ConnectionHandler {
    fn terminate(&self, error: io::Error) {
        self.state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .terminal = Some(error);
        self.state.control_event.notify_all();
        self.state
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending = false;
        self.state.read_event.notify_all();
        self.state
            .write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending = false;
        self.state.write_event.notify_all();
    }
}

impl BidirectionalStreamHandler for ConnectionHandler {
    fn on_stream_ready(&mut self, _stream: BidirectionalStreamHandle) {
        self.state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .ready = true;
        self.state.control_event.notify_all();
    }

    fn on_response_headers(
        &mut self,
        _stream: BidirectionalStreamHandle,
        headers: Vec<Header>,
        negotiated_protocol: String,
    ) {
        let mut state = self
            .state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.headers = Some(headers);
        state.negotiated_protocol = negotiated_protocol;
        self.state.control_event.notify_all();
    }

    fn on_read_completed(&mut self, _stream: BidirectionalStreamHandle, bytes_read: i32) {
        let mut read = self
            .state
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let count = usize::try_from(bytes_read).unwrap_or(0);
        read.completed = Some(count);
        read.eof = count == 0;
        read.pending = false;
        self.state.read_event.notify_all();
    }

    fn on_write_completed(&mut self, _stream: BidirectionalStreamHandle) {
        self.state
            .write
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pending = false;
        self.state.write_event.notify_all();
    }

    fn on_succeeded(&mut self, _stream: BidirectionalStreamHandle) {
        self.terminate(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "bidirectional stream completed",
        ));
    }

    fn on_failed(&mut self, _stream: BidirectionalStreamHandle, net_error: i32) {
        self.terminate(io::Error::other(format!("Chromium net error {net_error}")));
    }

    fn on_canceled(&mut self, _stream: BidirectionalStreamHandle) {
        self.terminate(io::Error::new(
            io::ErrorKind::Interrupted,
            "bidirectional stream canceled",
        ));
    }
}

fn clone_io_error(error: &io::Error) -> io::Error {
    io::Error::new(error.kind(), error.to_string())
}
