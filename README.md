# cronet-rs

Rust bindings for Chromium's Cronet native networking stack.

The workspace contains:

- `cronet-sys`: complete, version-matched raw bindings generated from the
  Cronet SDK's `cronet.idl_c.h`.
- `cronet`: an ownership and callback layer with engines, request builders,
  executors, response/error snapshots, upload providers, request status,
  blocking collection and runtime-independent async requests.

Cronet's generated C API is the stable native ABI that forwards to its C++
implementation. Binding that header therefore exposes both the supported C
surface and the underlying public native Cronet interfaces without relying on
the unstable C++ ABI.

## SDK setup

Build Cronet from Chromium or obtain a compatible binary SDK, then set:

```text
CRONET_INCLUDE_DIR=/path/to/sdk/include
CRONET_LIB_DIR=/path/to/sdk/lib
CRONET_LIB_NAME=cronet
```

`CRONET_INCLUDE_DIR` must contain `cronet_c.h` from a SagerNet SDK,
`cronet.idl_c.h`, or the Chromium-relative generated header. Place
`bidirectional_stream_c.h` in the same include search path when supplied by the
SDK. The directory containing `cronet_export.h` may be supplied separately
through `CRONET_EXPORT_INCLUDE_DIR`.

For editor work and documentation builds without a native SDK, the crate uses
a bundled ABI-compatible development header covering the safe wrapper and
prints a build warning. Set `CRONET_INCLUDE_DIR` for production so bindings are
complete for the exact binary being distributed.

```powershell
cargo check --workspace
cargo package -p cronet-sys
cargo package -p cronet
```

See [docs/distribution.md](docs/distribution.md) for binary distribution and
versioning rules.

## High-level usage

```rust,no_run
use cronet::{Client, Engine, EngineParams, RequestOptions};

let engine = Engine::start(&EngineParams::new())?;
let client = Client::new(engine)?;
let response = client.execute(
    RequestOptions::get("https://www.example.com")
        .header("Accept", "text/plain"),
)?;
println!("HTTP {}, {} bytes", response.info.status_code, response.body.len());
# Ok::<(), Box<dyn std::error::Error>>(())
```

For streaming applications, implement `UrlRequestHandler`; the crate supplies
the complete six-event callback trampoline and a dedicated-thread `Executor`.
`AsyncClient::execute` returns a runtime-independent Future.

See [API coverage](docs/api-coverage.md) for the mapping between Cronet native
interfaces and Rust types.

## Naive / SagerNet Cronet

The crate also binds the extensions used by
[`SagerNet/cronet-go`](https://github.com/SagerNet/cronet-go):

- `bidirectional_stream_*` H2/H3 streaming;
- custom TCP and UDP socket dialers;
- trusted-root injection and connection-pool shutdown;
- DNS, ECH, HTTP/2, QUIC and socket-pool experimental options;
- socket-like `BidirectionalConnection`;
- CONNECT headers and the first-eight-chunk Naive padding protocol through
  `NaiveConnection`.

These functions require a SagerNet/Naive Cronet binary. A stock Chromium Cronet
binary does not export all of the extension symbols. See
[`examples/naive_connect.rs`](crates/cronet/examples/naive_connect.rs).
