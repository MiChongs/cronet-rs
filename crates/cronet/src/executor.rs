use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    ptr::NonNull,
    sync::mpsc,
    thread,
};

use crate::sys;

/// A Cronet command submitted to an application executor.
pub struct Runnable(NonNull<sys::Cronet_Runnable>);

impl Runnable {
    /// Runs the command and releases its native object.
    pub fn run(self) {
        // SAFETY: This wrapper uniquely owns a live command.
        unsafe { sys::Cronet_Runnable_Run(self.0.as_ptr()) };
    }
}

impl Drop for Runnable {
    fn drop(&mut self) {
        // SAFETY: This wrapper uniquely owns the command.
        unsafe { sys::Cronet_Runnable_Destroy(self.0.as_ptr()) };
    }
}

// SAFETY: Cronet explicitly delegates a Runnable to an executor which may move it to
// another thread.
unsafe impl Send for Runnable {}

struct ExecutorState {
    dispatch: Box<dyn Fn(Runnable) + Send + Sync>,
    barrier: Option<Box<dyn Fn() + Send + Sync>>,
}

/// Application executor used by Cronet for callbacks.
pub struct Executor {
    raw: NonNull<sys::Cronet_Executor>,
    state: NonNull<ExecutorState>,
}

// SAFETY: The native executor context contains a Send + Sync dispatcher and is
// uniquely owned. Moving the adapter does not invalidate its heap context.
unsafe impl Send for Executor {}

impl Executor {
    /// Creates an executor from a command dispatcher.
    pub fn new(dispatch: impl Fn(Runnable) + Send + Sync + 'static) -> Self {
        Self::new_with_barrier(dispatch, None::<fn()>)
    }

    fn new_with_barrier(
        dispatch: impl Fn(Runnable) + Send + Sync + 'static,
        barrier: Option<impl Fn() + Send + Sync + 'static>,
    ) -> Self {
        let state = Box::new(ExecutorState {
            dispatch: Box::new(dispatch),
            barrier: barrier.map(|barrier| Box::new(barrier) as _),
        });
        let state = NonNull::from(Box::leak(state));
        // SAFETY: Trampoline has the exact generated C signature.
        let raw = unsafe { sys::Cronet_Executor_CreateWith(Some(execute_trampoline)) };
        let raw = NonNull::new(raw).expect("Cronet_Executor_CreateWith returned null");
        // SAFETY: Both pointers are live and context is recovered only by our trampoline.
        unsafe {
            sys::Cronet_Executor_SetClientContext(raw.as_ptr(), state.as_ptr().cast());
        }
        Self { raw, state }
    }

    /// Creates an executor backed by one dedicated Rust thread.
    pub fn dedicated_thread(name: impl Into<String>) -> std::io::Result<Self> {
        enum Command {
            Run(Runnable),
            Barrier(mpsc::SyncSender<()>),
        }

        let (sender, receiver) = mpsc::channel::<Command>();
        thread::Builder::new().name(name.into()).spawn(move || {
            while let Ok(command) = receiver.recv() {
                match command {
                    Command::Run(command) => {
                        command.run();
                    }
                    Command::Barrier(sender) => {
                        let _ = sender.send(());
                    }
                }
            }
        })?;
        let dispatch_sender = sender.clone();
        Ok(Self::new_with_barrier(
            move |command| {
                let _ = dispatch_sender.send(Command::Run(command));
            },
            Some(move || {
                let (completed, wait) = mpsc::sync_channel(0);
                if sender.send(Command::Barrier(completed)).is_ok() {
                    let _ = wait.recv();
                }
            }),
        ))
    }

    /// Executes callbacks immediately on the calling Cronet thread.
    ///
    /// Callback code must never block, perform I/O, or acquire contended locks.
    pub fn direct() -> Self {
        Self::new(Runnable::run)
    }

    /// Returns the native executor pointer.
    pub fn as_raw(&self) -> sys::Cronet_ExecutorPtr {
        self.raw.as_ptr()
    }

    pub(crate) fn wait_idle(&self) {
        // SAFETY: The state is heap-stable for the executor lifetime.
        if let Some(barrier) = &unsafe { self.state.as_ref() }.barrier {
            barrier();
        }
    }
}

impl Drop for Executor {
    fn drop(&mut self) {
        // SAFETY: Safe requests borrow this executor, so no callback can still
        // reference its context at normal destruction time.
        unsafe {
            sys::Cronet_Executor_SetClientContext(self.raw.as_ptr(), core::ptr::null_mut());
            sys::Cronet_Executor_Destroy(self.raw.as_ptr());
            drop(Box::from_raw(self.state.as_ptr()));
        }
    }
}

unsafe extern "C" fn execute_trampoline(
    executor: sys::Cronet_ExecutorPtr,
    command: sys::Cronet_RunnablePtr,
) {
    let Some(command) = NonNull::new(command) else {
        return;
    };
    // SAFETY: Context was installed by Executor::new and remains live because
    // safe requests borrow the executor.
    let context =
        unsafe { sys::Cronet_Executor_GetClientContext(executor) }.cast::<ExecutorState>();
    if context.is_null() {
        // SAFETY: No dispatcher accepted ownership.
        unsafe { sys::Cronet_Runnable_Destroy(command.as_ptr()) };
        return;
    }
    let runnable = Runnable(command);
    // Never unwind through Chromium C++ frames.
    let outcome = catch_unwind(AssertUnwindSafe(|| {
        // SAFETY: Non-null context was installed by Executor::new.
        unsafe { ((*context).dispatch)(runnable) };
    }));
    if outcome.is_err() {
        std::process::abort();
    }
}
