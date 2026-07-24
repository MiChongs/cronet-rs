use core::fmt;

/// An error returned by the native Cronet API.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Error {
    code: cronet_sys::Cronet_RESULT,
}

impl Error {
    /// Returns Cronet's original numeric result code.
    pub const fn code(self) -> cronet_sys::Cronet_RESULT {
        self.code
    }

    pub(crate) fn from_result(code: cronet_sys::Cronet_RESULT) -> Result<()> {
        if code == cronet_sys::Cronet_RESULT_Cronet_RESULT_SUCCESS {
            Ok(())
        } else {
            Err(Self { code })
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Cronet operation failed with result code {}", self.code)
    }
}

impl std::error::Error for Error {}

/// Result alias used by safe Cronet operations.
pub type Result<T> = core::result::Result<T, Error>;
