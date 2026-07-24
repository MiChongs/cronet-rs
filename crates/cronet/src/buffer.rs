use std::{ptr::NonNull, slice};

use crate::sys;

/// A buffer allocated and owned by Cronet.
pub struct Buffer(NonNull<sys::Cronet_Buffer>);

// SAFETY: A buffer has unique ownership and Cronet's API explicitly transfers its use
// between application executor and network threads. Shared access is not
// exposed, so it is Send but intentionally not Sync.
unsafe impl Send for Buffer {}

impl Buffer {
    /// Allocates an uninitialized native buffer.
    pub fn new(size: usize) -> Self {
        let size = u64::try_from(size).expect("buffer size does not fit uint64_t");
        // SAFETY: Native constructor and initializer accept these values.
        let ptr = unsafe { sys::Cronet_Buffer_Create() };
        let ptr = NonNull::new(ptr).expect("Cronet_Buffer_Create returned null");
        // SAFETY: `ptr` uniquely owns a newly created buffer.
        unsafe { sys::Cronet_Buffer_InitWithAlloc(ptr.as_ptr(), size) };
        Self(ptr)
    }

    /// Returns the buffer length.
    pub fn len(&self) -> usize {
        // SAFETY: Buffer is live.
        let len = unsafe { sys::Cronet_Buffer_GetSize(self.0.as_ptr()) };
        usize::try_from(len).expect("Cronet buffer is too large for this platform")
    }

    /// Reports whether the buffer has zero length.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the native pointer for request reads.
    pub fn as_raw(&self) -> sys::Cronet_BufferPtr {
        self.0.as_ptr()
    }
}

impl AsRef<[u8]> for Buffer {
    fn as_ref(&self) -> &[u8] {
        let len = self.len();
        if len == 0 {
            return &[];
        }
        // SAFETY: Cronet promises GetData points to GetSize bytes owned by this
        // live buffer; shared borrow prevents mutable Rust access.
        unsafe { slice::from_raw_parts(sys::Cronet_Buffer_GetData(self.0.as_ptr()).cast(), len) }
    }
}

impl AsMut<[u8]> for Buffer {
    fn as_mut(&mut self) -> &mut [u8] {
        let len = self.len();
        if len == 0 {
            return &mut [];
        }
        // SAFETY: Unique borrow prevents aliases through this safe wrapper.
        unsafe {
            slice::from_raw_parts_mut(sys::Cronet_Buffer_GetData(self.0.as_ptr()).cast(), len)
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the native object.
        unsafe { sys::Cronet_Buffer_Destroy(self.0.as_ptr()) };
    }
}
