# API coverage

## Generated native layer

`cronet-sys` allowlists every function, type and constant beginning with
`Cronet_` from the selected SDK header. The bundled development ABI is now the
generated header from the NaiveProxy source pinned by
`SagerNet/cronet-go@d62042e935130168f4cebcd4515319a88ee7abcf`.

An integration test checks all 255 symbols loaded by that `cronet-go` commit
against the bundled Cronet and bidirectional-stream declarations. Updating the
pin requires reviewing the machine-readable ABI manifest in
`crates/cronet-sys/abi`.

The C API is Cronet's generated forwarding ABI over its native C++
implementation. Direct binding to Chromium C++ classes is deliberately avoided
because their names, layout, standard library ABI and ownership types are not a
stable binary interface.

## Safe operational layer

| Cronet interface | Rust surface |
|---|---|
| Engine / EngineParams | `Engine`, `EngineParams` |
| QUIC hints and public-key pins | `EngineParams::quic_hint`, `public_key_pins` |
| Runnable / Executor | `Runnable`, `Executor` |
| Buffer | `Buffer` |
| BufferCallback / external data | `Buffer::from_vec` |
| UrlRequestParams / HttpHeader | `RequestParams`, `Header` |
| UrlRequest | `UninitializedRequest`, `Request`, `RequestHandle` |
| UrlRequestCallback | `UrlRequestHandler`, `UrlRequestCallback` |
| UrlRequestStatusListener | `Request::status`, `RequestStatus` |
| UrlResponseInfo | owned `ResponseInfo` snapshot |
| Error | owned `NetworkError` snapshot |
| Metrics / RequestFinishedInfo | owned `Metrics`, `RequestFinishedInfo` snapshots |
| RequestFinishedInfoListener | `Engine::add_request_finished_listener` |
| UploadDataProvider / UploadDataSink | `UploadDataProvider` |
| Complete request collection | `Client`, `RequestOptions`, `Response` |
| Future-based execution | `AsyncClient` |
| Bidirectional H2/H3 stream | `BidirectionalStream`, `BidirectionalStreamHandler` |
| Socket-like duplex stream | `BidirectionalConnection` |
| Custom TCP/UDP dialing | `NetworkHooks`, `UdpDialResult` |
| Naive CONNECT + padding | `NaiveConnection`, `NaiveConnectOptions` |
| Naive client and pool isolation | `NaiveClient`, `NaiveClientOptions` |

## SagerNet/Naive extensions

The SagerNet Cronet fork adds capabilities outside the stock generated Cronet
IDL. `cronet-sys` binds `Cronet_Engine_GetStreamEngine`, custom TCP/UDP dialers,
trusted roots, connection-pool shutdown and the complete
`bidirectional_stream_c.h` surface. The safe layer preserves the native rule of
at most one read and one write in flight while permitting those directions to
run concurrently.

`NaiveConnection` implements the CONNECT metadata conventions and the
first-eight-read/write padding records used by NaiveProxy. Applications may
perform the handshake separately from stream startup for Fast Open behavior.

`NaiveClient` now owns engine startup, Basic authentication, H2/H3 defaults,
connection-pool isolation and safe shutdown. DNS interception, ECH resolver
adaptation and platform socket-pair transport remain implementation work and
are not yet claimed as covered.

Callback panics never unwind through native frames. Callback, executor, engine,
request, upload and buffer lifetimes are tied together by Rust ownership in the
safe entry points. Low-level constructors for mocks, annotations and
SDK-version-specific additions remain available through `cronet::sys`.
