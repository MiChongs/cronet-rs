# Distribution

`cronet` and `cronet-sys` are published as ordinary crates.io dependencies.
Native SagerNet Cronet is distributed separately because each artifact is
platform-specific and roughly 8–16 MiB before packaging.

## Pinned native release

The supported prebuilt set is SagerNet Cronet `v148.0.7778.96-1`. The exact
SHA-256 digest for every upstream Linux and Windows asset is committed in
[`native/SHA256SUMS`](../native/SHA256SUMS). The fetch helpers refuse a
mismatched download:

```powershell
./scripts/fetch-native.ps1
```

```sh
./scripts/fetch-native.sh
```

The PowerShell helper also generates the MSVC-compatible `cronet.lib` import
library from the pinned 255-symbol ABI manifest. It requires `llvm-dlltool` or
GNU `dlltool`. The shell helper stages the selected Linux shared object as
`libcronet.so`.

SagerNet does not publish macOS assets in that release. macOS consumers must
build the pinned NaiveProxy/Cronet revision and provide its include and library
directories explicitly.

## Cargo build configuration

Set:

```text
CRONET_INCLUDE_DIR=/path/to/sdk/include
CRONET_EXPORT_INCLUDE_DIR=/optional/path/to/cronet_export
CRONET_LIB_DIR=/path/to/sdk/lib
CRONET_LIB_NAME=cronet
```

`CRONET_INCLUDE_DIR` is optional when using the exact pinned SagerNet binary:
the crate contains its matching development declarations. It is required when
linking another Cronet revision so bindgen reads that SDK's real declarations.
Set `CRONET_STATIC=1` only for a static library and ensure all of Chromium's
native link dependencies are also supplied.

The runtime loader must be able to find `cronet.dll`, `libcronet.so` or
`libcronet.dylib`. The helper output shows the required `PATH` or
`LD_LIBRARY_PATH` entry.

## Release verification

The source CI builds, lints and tests on Linux, Windows and macOS. The native
workflow separately downloads the checksummed SagerNet binaries on Linux and
Windows and runs:

- native buffer allocation/destruction;
- an HTTPS request with status 200;
- custom DNS interception;
- a bidirectional CONNECT request and deterministic shutdown.

Before publishing a source release:

```text
cargo fmt --all -- --check
cargo test --workspace --all-targets
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo package -p cronet-sys
cargo package -p cronet
```

Publish `cronet-sys` first, wait for its crates.io index entry, then publish
`cronet`. Native binaries are not embedded in either crate.

The Rust wrapper is GPL-3.0-or-later to remain compatible with the
`SagerNet/cronet-go` implementation used for behavioral parity. Generated
Chromium declarations retain their upstream BSD notices; see
[`THIRD_PARTY_NOTICES.md`](../THIRD_PARTY_NOTICES.md).
