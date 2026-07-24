# cronet

Ownership-aware Rust wrappers over `cronet-sys`. The native callback interfaces
remain available from `cronet::sys`; higher-level async adapters can be built
without losing access to any Cronet API.

