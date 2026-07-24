# Changelog

## 0.2.0 - 2026-07-25

This release aligns the Rust implementation with
`SagerNet/cronet-go@d62042e935130168f4cebcd4515319a88ee7abcf` and its pinned
NaiveProxy tree.

### Native and safe API

- Cover all 255 native symbols used by the pinned `cronet-go`, including the
  generated Cronet ABI, SagerNet engine extensions and bidirectional streams.
- Add owned metrics, finished-info/listener, buffer-callback and external-buffer
  wrappers.
- Correct asynchronous Cronet buffer ownership and executor/request teardown.
- Preserve partial native stream reads and implement read/write/header
  deadlines.
- Correct C++ boolean return handling for native bidirectional reads and writes.

### Naive, DNS and transport

- Add engine-owning `NaiveClient`, Basic authentication, H2/H3 defaults,
  connection-pool isolation, CONNECT metadata and first-eight-chunk padding.
- Add DNS-over-TCP/UDP interception, fixed-address redirect and system fallback.
- Add fixed or queried ECHConfig injection, HTTPS/SVCB name rewriting, service
  ports, ALPN handling and IP-hint filtering.
- Transfer native TCP/UDP sockets on Unix and Windows with Chromium net-error
  mapping.
- Bridge arbitrary Rust split streams and callback-backed datagram transports
  while retaining duplex and packet-boundary semantics.

### Distribution and verification

- Default to checked-in pinned bindings, removing libclang from normal
  downstream builds; retain opt-in regeneration for external SDKs.
- Add checksummed SagerNet native fetch/staging helpers for all published Linux
  and Windows targets.
- Add source CI on Linux, Windows and macOS plus native Linux/Windows tests.
- Validate HTTPS, custom DNS, rejected CONNECT, successful TLS/H2 Naive
  CONNECT, all eight padded chunks, an unpadded ninth chunk and deterministic
  shutdown against the real native library.
- License this compatibility implementation as GPL-3.0-or-later and record
  upstream notices.
