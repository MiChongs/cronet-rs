use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::{NonNull, copy_nonoverlapping},
    sync::Mutex,
};

use crate::sys;

struct UploadState {
    bytes: Vec<u8>,
    position: usize,
    closed: bool,
}

/// Rewindable in-memory request body provider.
pub struct UploadDataProvider {
    raw: NonNull<sys::Cronet_UploadDataProvider>,
    state: NonNull<Mutex<UploadState>>,
}

impl UploadDataProvider {
    /// Creates an upload provider that owns `bytes`.
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Self {
        let state = Box::new(Mutex::new(UploadState {
            bytes: bytes.into(),
            position: 0,
            closed: false,
        }));
        let state = NonNull::from(Box::leak(state));
        // SAFETY: Trampolines use the generated C signatures.
        let raw = unsafe {
            sys::Cronet_UploadDataProvider_CreateWith(
                Some(get_length),
                Some(read),
                Some(rewind),
                Some(close),
            )
        };
        let raw = NonNull::new(raw).expect("Cronet_UploadDataProvider_CreateWith returned null");
        // SAFETY: Context is uniquely owned and remains live with the provider.
        unsafe {
            sys::Cronet_UploadDataProvider_SetClientContext(raw.as_ptr(), state.as_ptr().cast());
        }
        Self { raw, state }
    }

    /// Returns the native upload provider.
    pub fn as_raw(&self) -> sys::Cronet_UploadDataProviderPtr {
        self.raw.as_ptr()
    }
}

impl Drop for UploadDataProvider {
    fn drop(&mut self) {
        // SAFETY: Safe high-level requests keep the provider live until request
        // completion and no callback can access its context afterward.
        unsafe {
            sys::Cronet_UploadDataProvider_SetClientContext(
                self.raw.as_ptr(),
                core::ptr::null_mut(),
            );
            sys::Cronet_UploadDataProvider_Destroy(self.raw.as_ptr());
            drop(Box::from_raw(self.state.as_ptr()));
        }
    }
}

unsafe fn state(
    provider: sys::Cronet_UploadDataProviderPtr,
) -> Option<&'static Mutex<UploadState>> {
    // SAFETY: Provider context was installed by from_bytes.
    let raw = unsafe { sys::Cronet_UploadDataProvider_GetClientContext(provider) }
        .cast::<Mutex<UploadState>>();
    // SAFETY: Non-null context lives as long as the native provider.
    unsafe { raw.as_ref() }
}

unsafe extern "C" fn get_length(provider: sys::Cronet_UploadDataProviderPtr) -> i64 {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Cronet invokes this only on the provider it received.
        let Some(state) = (unsafe { state(provider) }) else {
            return -1;
        };
        let state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        i64::try_from(state.bytes.len()).unwrap_or(-1)
    }));
    outcome.unwrap_or_else(|_| std::process::abort())
}

unsafe extern "C" fn read(
    provider: sys::Cronet_UploadDataProviderPtr,
    sink: sys::Cronet_UploadDataSinkPtr,
    buffer: sys::Cronet_BufferPtr,
) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Cronet supplies live provider, sink and buffer objects.
        let Some(state) = (unsafe { state(provider) }) else {
            return;
        };
        let mut state = state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // SAFETY: Buffer is live for the callback.
        let capacity = unsafe { sys::Cronet_Buffer_GetSize(buffer) };
        let capacity = usize::try_from(capacity).unwrap_or(usize::MAX);
        let remaining = state.bytes.len().saturating_sub(state.position);
        let count = capacity.min(remaining);
        if count != 0 {
            // SAFETY: Source has `count` remaining bytes and Cronet buffer has
            // at least `count` writable bytes.
            unsafe {
                copy_nonoverlapping(
                    state.bytes.as_ptr().add(state.position),
                    sys::Cronet_Buffer_GetData(buffer).cast(),
                    count,
                );
            }
            state.position += count;
        }
        let final_chunk = state.position == state.bytes.len();
        // SAFETY: Sink is live for this callback.
        unsafe {
            sys::Cronet_UploadDataSink_OnReadSucceeded(
                sink,
                u64::try_from(count).unwrap_or(u64::MAX),
                final_chunk,
            )
        };
    }));
    if outcome.is_err() {
        std::process::abort();
    }
}

unsafe extern "C" fn rewind(
    provider: sys::Cronet_UploadDataProviderPtr,
    sink: sys::Cronet_UploadDataSinkPtr,
) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Cronet invokes this only on the provider it received.
        let Some(state) = (unsafe { state(provider) }) else {
            return;
        };
        state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .position = 0;
        // SAFETY: Sink is live for this callback.
        unsafe { sys::Cronet_UploadDataSink_OnRewindSucceeded(sink) };
    }));
    if outcome.is_err() {
        std::process::abort();
    }
}

unsafe extern "C" fn close(provider: sys::Cronet_UploadDataProviderPtr) {
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Cronet invokes this only on the provider it received.
        if let Some(state) = unsafe { state(provider) } {
            state
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .closed = true;
        }
    }));
    if outcome.is_err() {
        std::process::abort();
    }
}
