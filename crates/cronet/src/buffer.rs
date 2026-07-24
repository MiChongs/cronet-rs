use std::{cell::Cell, ptr::NonNull, slice};

use crate::sys;

/// A buffer allocated and owned by Cronet.
pub struct Buffer {
    raw: NonNull<sys::Cronet_Buffer>,
    callback: Option<NonNull<sys::Cronet_BufferCallback>>,
    native_owned: Cell<bool>,
}

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
        Self {
            raw: ptr,
            callback: None,
            native_owned: Cell::new(false),
        }
    }

    /// Creates a Cronet buffer backed by Rust-owned bytes without copying.
    ///
    /// The byte allocation is released by Cronet's buffer-destruction
    /// callback, including when destruction occurs on a native worker thread.
    pub fn from_vec(bytes: Vec<u8>) -> Self {
        let mut bytes = bytes.into_boxed_slice();
        let data = bytes.as_mut_ptr();
        let size = u64::try_from(bytes.len()).expect("buffer size does not fit uint64_t");
        // A second box turns the slice's fat pointer into a thin context
        // pointer that can cross the C ABI.
        let state = Box::into_raw(Box::new(bytes));

        // SAFETY: Constructor and callback signature come from the pinned IDL.
        let callback = unsafe { sys::Cronet_BufferCallback_CreateWith(Some(release_rust_buffer)) };
        let callback =
            NonNull::new(callback).expect("Cronet_BufferCallback_CreateWith returned null");
        // SAFETY: Context remains allocated until release_rust_buffer or the
        // defensive cleanup in Drop.
        unsafe {
            sys::Cronet_BufferCallback_SetClientContext(
                callback.as_ptr(),
                state.cast::<core::ffi::c_void>(),
            );
        }

        // SAFETY: The boxed slice and callback are live and retained by this
        // wrapper until Cronet destroys the buffer.
        let raw = unsafe { sys::Cronet_Buffer_Create() };
        let raw = NonNull::new(raw).expect("Cronet_Buffer_Create returned null");
        // SAFETY: Data, callback and raw buffer are all live for initialization.
        unsafe {
            sys::Cronet_Buffer_InitWithDataAndCallback(
                raw.as_ptr(),
                data.cast(),
                size,
                callback.as_ptr(),
            );
        }
        Self {
            raw,
            callback: Some(callback),
            native_owned: Cell::new(false),
        }
    }

    /// Returns the buffer length.
    pub fn len(&self) -> usize {
        // SAFETY: Buffer is live.
        let len = unsafe { sys::Cronet_Buffer_GetSize(self.raw.as_ptr()) };
        usize::try_from(len).expect("Cronet buffer is too large for this platform")
    }

    /// Reports whether the buffer has zero length.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the native pointer for request reads.
    pub fn as_raw(&self) -> sys::Cronet_BufferPtr {
        self.raw.as_ptr()
    }

    pub(crate) fn transfer_to_native(&self) {
        self.native_owned.set(true);
    }

    /// Marks a buffer returned by `OnReadCompleted` as application-owned.
    ///
    /// Returns `false` when `raw` is not this buffer.
    pub fn reclaim_from_read(&self, raw: sys::Cronet_BufferPtr) -> bool {
        if self.raw.as_ptr() != raw {
            return false;
        }
        self.native_owned.set(false);
        true
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
        unsafe { slice::from_raw_parts(sys::Cronet_Buffer_GetData(self.raw.as_ptr()).cast(), len) }
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
            slice::from_raw_parts_mut(sys::Cronet_Buffer_GetData(self.raw.as_ptr()).cast(), len)
        }
    }
}

impl Drop for Buffer {
    fn drop(&mut self) {
        if !self.native_owned.get() {
            // SAFETY: This wrapper uniquely owns the native object. Destroy
            // invokes the optional data callback before returning.
            unsafe { sys::Cronet_Buffer_Destroy(self.raw.as_ptr()) };
        }
        if let Some(callback) = self.callback.take() {
            // SAFETY: A conforming Cronet clears the context in our callback.
            // If initialization failed to invoke it, reclaim the Rust slice
            // defensively before destroying the callback object.
            unsafe {
                let context = sys::Cronet_BufferCallback_GetClientContext(callback.as_ptr())
                    .cast::<Box<[u8]>>();
                if !context.is_null() {
                    sys::Cronet_BufferCallback_SetClientContext(
                        callback.as_ptr(),
                        core::ptr::null_mut(),
                    );
                    drop(Box::from_raw(context));
                }
                sys::Cronet_BufferCallback_Destroy(callback.as_ptr());
            }
        }
    }
}

unsafe extern "C" fn release_rust_buffer(
    callback: sys::Cronet_BufferCallbackPtr,
    _buffer: sys::Cronet_BufferPtr,
) {
    if callback.is_null() {
        return;
    }
    // SAFETY: Buffer::from_vec installed a Box<Box<[u8]>> as this callback's
    // unique context.
    let context =
        unsafe { sys::Cronet_BufferCallback_GetClientContext(callback) }.cast::<Box<[u8]>>();
    if context.is_null() {
        return;
    }
    // SAFETY: Clear the live context first so defensive Drop cleanup cannot
    // release it twice, then recover its unique Box ownership.
    unsafe {
        sys::Cronet_BufferCallback_SetClientContext(callback, core::ptr::null_mut());
        drop(Box::from_raw(context));
    }
}
