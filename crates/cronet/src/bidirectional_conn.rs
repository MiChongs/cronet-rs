use std::{
    io::{self, Read, Write},
    sync::{Arc, Condvar, Mutex, MutexGuard},
    time::{Duration, Instant},
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
    offset: usize,
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
    read_deadline: Mutex<Option<Instant>>,
    write_deadline: Mutex<Option<Instant>>,
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
                offset: 0,
                eof: false,
            }),
            read_event: Condvar::new(),
            write: Mutex::new(WriteState::default()),
            write_event: Condvar::new(),
            read_deadline: Mutex::new(None),
            write_deadline: Mutex::new(None),
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
        let deadline = self.read_deadline();
        let control = self
            .state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (control, timed_out) =
            wait_until(&self.state.control_event, control, deadline, |state| {
                state.headers.is_none() && state.terminal.is_none()
            });
        if timed_out {
            self.cancel();
            return Err(timeout_error("waiting for response headers"));
        }
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
        self.wait_ready_until(self.read_deadline())
    }

    fn wait_ready_until(&self, deadline: Option<Instant>) -> io::Result<()> {
        let control = self
            .state
            .control
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (control, timed_out) =
            wait_until(&self.state.control_event, control, deadline, |state| {
                !state.ready && state.terminal.is_none()
            });
        if timed_out {
            self.cancel();
            return Err(timeout_error("waiting for stream readiness"));
        }
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
        let deadline = self.read_deadline();
        self.wait_ready_until(deadline)?;
        let mut read = self
            .state
            .read
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(copied) = copy_available(&mut read, output) {
            return Ok(copied);
        }
        if read.eof {
            return Ok(0);
        }
        read.completed = None;
        read.offset = 0;
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
        let (mut read, timed_out) = wait_until(&self.state.read_event, read, deadline, |state| {
            state.pending
        });
        if timed_out {
            drop(read);
            self.cancel();
            return Err(timeout_error("reading bidirectional stream"));
        }
        if let Some(error) = self.terminal_error() {
            return Err(error);
        }
        Ok(copy_available(&mut read, output).unwrap_or(0))
    }

    /// Writes request data using a separately synchronized write direction.
    pub fn write_shared(&self, input: &[u8], end_of_stream: bool) -> io::Result<usize> {
        let deadline = self.write_deadline();
        self.wait_ready_until(deadline)?;
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
        let (write, timed_out) = wait_until(&self.state.write_event, write, deadline, |state| {
            state.pending
        });
        drop(write);
        if timed_out {
            self.cancel();
            return Err(timeout_error("writing bidirectional stream"));
        }
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

    /// Sets an absolute deadline for subsequent reads and header waits.
    pub fn set_read_deadline(&self, deadline: Option<Instant>) {
        *self
            .state
            .read_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = deadline;
        self.state.control_event.notify_all();
        self.state.read_event.notify_all();
    }

    /// Sets an absolute deadline for subsequent writes.
    pub fn set_write_deadline(&self, deadline: Option<Instant>) {
        *self
            .state
            .write_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = deadline;
        self.state.control_event.notify_all();
        self.state.write_event.notify_all();
    }

    /// Sets both read and write deadlines.
    pub fn set_deadline(&self, deadline: Option<Instant>) {
        self.set_read_deadline(deadline);
        self.set_write_deadline(deadline);
    }

    /// Sets a relative read timeout. `None` disables it.
    pub fn set_read_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_read_deadline(timeout_deadline(timeout)?);
        Ok(())
    }

    /// Sets a relative write timeout. `None` disables it.
    pub fn set_write_timeout(&self, timeout: Option<Duration>) -> io::Result<()> {
        self.set_write_deadline(timeout_deadline(timeout)?);
        Ok(())
    }

    fn read_deadline(&self) -> Option<Instant> {
        *self
            .state
            .read_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn write_deadline(&self) -> Option<Instant> {
        *self
            .state
            .write_deadline
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        read.offset = 0;
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

fn wait_until<'a, T>(
    event: &Condvar,
    mut state: MutexGuard<'a, T>,
    deadline: Option<Instant>,
    waiting: impl Fn(&T) -> bool,
) -> (MutexGuard<'a, T>, bool) {
    loop {
        if !waiting(&state) {
            return (state, false);
        }
        let Some(deadline) = deadline else {
            state = event
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            continue;
        };
        let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
            return (state, true);
        };
        let (next, timeout) = event
            .wait_timeout(state, remaining)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state = next;
        if timeout.timed_out() && waiting(&state) {
            return (state, true);
        }
    }
}

fn timeout_deadline(timeout: Option<Duration>) -> io::Result<Option<Instant>> {
    match timeout {
        Some(timeout) if timeout.is_zero() => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "zero timeout is invalid",
        )),
        Some(timeout) => Instant::now()
            .checked_add(timeout)
            .map(Some)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "timeout is too large")),
        None => Ok(None),
    }
}

fn timeout_error(operation: &str) -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        format!("timed out while {operation}"),
    )
}

fn copy_available(read: &mut ReadState, output: &mut [u8]) -> Option<usize> {
    let count = read.completed?.min(read.buffer.len());
    let remaining = count.saturating_sub(read.offset);
    let copied = remaining.min(output.len());
    output[..copied].copy_from_slice(&read.buffer[read.offset..read.offset + copied]);
    read.offset += copied;
    if read.offset == count {
        read.completed = None;
        read.offset = 0;
    }
    Some(copied)
}

#[cfg(test)]
mod tests {
    use super::{ReadState, copy_available};

    #[test]
    fn preserves_unconsumed_native_read_bytes() {
        let mut read = ReadState {
            buffer: b"abcdefgh".to_vec().into_boxed_slice(),
            pending: false,
            completed: Some(8),
            offset: 0,
            eof: false,
        };
        let mut first = [0_u8; 3];
        let mut second = [0_u8; 5];
        assert_eq!(copy_available(&mut read, &mut first), Some(3));
        assert_eq!(&first, b"abc");
        assert_eq!(copy_available(&mut read, &mut second), Some(5));
        assert_eq!(&second, b"defgh");
        assert_eq!(read.completed, None);
    }
}
