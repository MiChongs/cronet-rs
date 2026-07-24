# cronet

Ownership-aware Rust wrappers over `cronet-sys`. The native callback interfaces
remain available from `cronet::sys`; higher-level async adapters can be built
without losing access to any Cronet API.

The safe API includes URL requests, H2/H3 bidirectional streams, Naive CONNECT
padding, custom TCP/UDP transports, DNS interception and ECH adaptation.
Checksummed native-library setup helpers are provided by the workspace
repository.
