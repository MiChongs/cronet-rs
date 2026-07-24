use std::{
    future::Future,
    sync::mpsc,
    thread::{self, JoinHandle},
};

use futures_channel::oneshot;

use crate::{Client, RequestError, RequestOptions, Response};

type Job = (
    RequestOptions,
    oneshot::Sender<Result<Response, RequestError>>,
);

/// Runtime-independent asynchronous Cronet client.
///
/// A dedicated worker owns the synchronous client, while each result is
/// delivered through a lightweight Future that can be awaited by any executor.
pub struct AsyncClient {
    sender: Option<mpsc::Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}

impl AsyncClient {
    /// Starts an async worker around `client`.
    pub fn new(client: Client) -> std::io::Result<Self> {
        let (sender, receiver) = mpsc::channel::<Job>();
        let worker = thread::Builder::new()
            .name("cronet-rs-client".into())
            .spawn(move || {
                while let Ok((options, completion)) = receiver.recv() {
                    let _ = completion.send(client.execute(options));
                }
            })?;
        Ok(Self {
            sender: Some(sender),
            worker: Some(worker),
        })
    }

    /// Schedules a request and returns a runtime-independent Future.
    pub fn execute(
        &self,
        options: RequestOptions,
    ) -> impl Future<Output = Result<Response, RequestError>> + Send + 'static {
        let (completion, receiver) = oneshot::channel();
        let sent = self
            .sender
            .as_ref()
            .is_some_and(|sender| sender.send((options, completion)).is_ok());
        async move {
            if !sent {
                return Err(RequestError::CallbackDisconnected);
            }
            receiver
                .await
                .unwrap_or(Err(RequestError::CallbackDisconnected))
        }
    }
}

impl Drop for AsyncClient {
    fn drop(&mut self) {
        drop(self.sender.take());
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
