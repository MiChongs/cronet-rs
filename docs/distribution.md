# Distribution

`cronet` and `cronet-sys` are intended to be published as ordinary crates.io
dependencies. The Cronet binary is not redistributed by default because its
platform artifacts, transitive native dependencies and Chromium notices vary
by target.

Applications may either:

1. install a Cronet SDK and set the three `CRONET_*` variables while building;
2. have a platform package place the import library and runtime library in
   standard search locations; or
3. make an internal wrapper crate that downloads a pinned, checksummed SDK and
   sets Cargo link metadata.

Always pin the Chromium/Cronet revision. `cronet-sys` regenerates every symbol
whose name begins with `Cronet_` from that SDK's generated C header. This avoids
mixing declarations from one Cronet release with a binary from another.

For crates.io publishing, replace the placeholder workspace `repository`
field, add the Chromium `LICENSE`/notices required by the binary package, run:

```text
cargo package -p cronet-sys
cargo package -p cronet
```

The safe crate follows semantic versioning. Changes in the selected Cronet SDK
ABI should also cause at least a minor release of `cronet-sys`.

