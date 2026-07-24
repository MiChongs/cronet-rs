use std::{fmt, io};

/// Chromium `net::Error` value used by SagerNet socket callbacks.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct NetError(i32);

impl NetError {
    /// Operation was canceled.
    pub const ABORTED: Self = Self(-3);
    /// Generic timeout.
    pub const TIMED_OUT: Self = Self(-7);
    /// Peer closed the connection.
    pub const CONNECTION_CLOSED: Self = Self(-100);
    /// Connection was reset.
    pub const CONNECTION_RESET: Self = Self(-101);
    /// Connection was refused.
    pub const CONNECTION_REFUSED: Self = Self(-102);
    /// Connection was aborted.
    pub const CONNECTION_ABORTED: Self = Self(-103);
    /// Generic connection failure.
    pub const CONNECTION_FAILED: Self = Self(-104);
    /// Address or network is unreachable.
    pub const ADDRESS_UNREACHABLE: Self = Self(-109);
    /// Connection establishment timed out.
    pub const CONNECTION_TIMED_OUT: Self = Self(-118);

    /// Returns the raw Chromium error number.
    pub const fn code(self) -> i32 {
        self.0
    }

    /// Maps a Rust I/O error to the value expected by Cronet's custom dialers.
    pub fn from_io(error: &io::Error) -> Self {
        if error.kind() == io::ErrorKind::TimedOut {
            return Self::CONNECTION_TIMED_OUT;
        }
        match error.kind() {
            io::ErrorKind::ConnectionRefused => Self::CONNECTION_REFUSED,
            io::ErrorKind::ConnectionReset => Self::CONNECTION_RESET,
            io::ErrorKind::ConnectionAborted => Self::CONNECTION_ABORTED,
            io::ErrorKind::NotConnected => Self::CONNECTION_CLOSED,
            io::ErrorKind::AddrNotAvailable | io::ErrorKind::HostUnreachable => {
                Self::ADDRESS_UNREACHABLE
            }
            io::ErrorKind::Interrupted => Self::ABORTED,
            _ => Self::CONNECTION_FAILED,
        }
    }
}

impl fmt::Display for NetError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "Chromium network error {}", self.0)
    }
}

impl std::error::Error for NetError {}

#[cfg(test)]
mod tests {
    use std::io;

    use super::NetError;

    #[test]
    fn maps_common_io_errors() {
        assert_eq!(
            NetError::from_io(&io::Error::from(io::ErrorKind::ConnectionRefused)),
            NetError::CONNECTION_REFUSED
        );
        assert_eq!(
            NetError::from_io(&io::Error::from(io::ErrorKind::TimedOut)),
            NetError::CONNECTION_TIMED_OUT
        );
    }
}
