# API coverage

## Generated native layer

`cronet-sys` allowlists every function, type and constant beginning with
`Cronet_` from the selected SDK header. This covers concrete and abstract
interfaces, generated struct accessors and testing/mock constructors. A checked
Chromium header currently produces 199 function declarations.

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
| UrlRequestParams / HttpHeader | `RequestParams`, `Header` |
| UrlRequest | `UninitializedRequest`, `Request`, `RequestHandle` |
| UrlRequestCallback | `UrlRequestHandler`, `UrlRequestCallback` |
| UrlRequestStatusListener | `Request::status`, `RequestStatus` |
| UrlResponseInfo | owned `ResponseInfo` snapshot |
| Error | owned `NetworkError` snapshot |
| UploadDataProvider / UploadDataSink | `UploadDataProvider` |
| Complete request collection | `Client`, `RequestOptions`, `Response` |
| Future-based execution | `AsyncClient` |
| Bidirectional H2/H3 stream | `BidirectionalStream`, `BidirectionalStreamHandler` |
| Socket-like duplex stream | `BidirectionalConnection` |
| Custom TCP/UDP dialing | `NetworkHooks`, `UdpDialResult` |
| Naive CONNECT + padding | `NaiveConnection`, `NaiveConnectOptions` |

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

Callback panics never unwind through native frames. Callback, executor, engine,
request, upload and buffer lifetimes are tied together by Rust ownership in the
safe entry points. Low-level constructors for mocks, annotations and
SDK-version-specific additions remain available through `cronet::sys`.
