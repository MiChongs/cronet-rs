# cronet-rs

Rust bindings for Chromium's Cronet native networking stack.

The safe layer is designed for SagerNet/NaiveProxy use and is aligned with
`SagerNet/cronet-go@d62042e935130168f4cebcd4515319a88ee7abcf`.

The workspace contains:

- `cronet-sys`: raw bindings generated from the selected Cronet SDK, with a
  bundled development ABI pinned to `SagerNet/cronet-go@d62042e`.
- `cronet`: an ownership and callback layer with engines, request builders,
  executors, response/error snapshots, upload providers, request status,
  blocking collection and runtime-independent async requests.

Cronet's generated C API is the stable native ABI that forwards to its C++
implementation. Binding that header therefore exposes both the supported C
surface and the underlying public native Cronet interfaces without relying on
the unstable C++ ABI.

## SDK setup

For the pinned SagerNet build on Windows:

```powershell
./scripts/fetch-native.ps1
$env:CRONET_LIB_DIR = "$PWD/target/cronet-sdk/lib"
$env:CRONET_LIB_NAME = "cronet"
$env:PATH = "$PWD/target/cronet-sdk/bin;$env:PATH"
```

On Linux:

```sh
./scripts/fetch-native.sh
export CRONET_LIB_DIR="$PWD/target/cronet-sdk/lib"
export CRONET_LIB_NAME=cronet
export LD_LIBRARY_PATH="$CRONET_LIB_DIR${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
```

Both helpers verify the pinned upstream asset against
[`native/SHA256SUMS`](native/SHA256SUMS). Alternatively, build Cronet from
Chromium/NaiveProxy or supply a compatible SDK, then set:

```text
CRONET_INCLUDE_DIR=/path/to/sdk/include
CRONET_LIB_DIR=/path/to/sdk/lib
CRONET_LIB_NAME=cronet
```

When using `CRONET_INCLUDE_DIR`, enable the `cronet-sys/generate-bindings`
feature and install libclang. The directory must contain `cronet_c.h` from a
SagerNet SDK, `cronet.idl_c.h`, or the Chromium-relative generated header. Place
`bidirectional_stream_c.h` in the same include search path when supplied by the
SDK. The directory containing `cronet_export.h` may be supplied separately
through `CRONET_EXPORT_INCLUDE_DIR`.

For editor work and documentation builds without a native SDK, the crate uses
the generated Cronet header and bidirectional-stream header from the pinned
NaiveProxy source. A test checks all 255 native symbols loaded by the matching
`cronet-go` commit. Set `CRONET_INCLUDE_DIR` for production so bindings are
generated for the exact binary being distributed.

```powershell
cargo check --workspace
cargo package -p cronet-sys
cargo package -p cronet
```

See [docs/distribution.md](docs/distribution.md) for binary distribution and
versioning rules, and [CHANGELOG.md](CHANGELOG.md) for release changes.

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
- engine-owning `NaiveClient` with Basic authentication and connection-pool
  isolation;
- CONNECT headers and the first-eight-chunk Naive padding protocol through
  `NaiveConnection`.

These functions require a SagerNet/Naive Cronet binary. A stock Chromium Cronet
binary does not export all of the extension symbols. See
[`examples/naive_connect.rs`](crates/cronet/examples/naive_connect.rs).

DNS interception, ECH resolver adaptation, platform socket forwarding and
prebuilt native-library acquisition are implemented for the SagerNet release
targets listed in [`native/SHA256SUMS`](native/SHA256SUMS). Custom Rust byte
streams and packet transports can be adapted with `SplitStream` and
`SplitDatagram`.

The raw layer exposes every one of the 255 symbols used by the pinned
`cronet-go`; the safe layer covers the lifecycle, request, streaming, DNS, ECH
and Naive client paths. Platform-specific symbols added by a future SDK remain
available through `cronet::sys`.
