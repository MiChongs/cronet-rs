# cronet-sys

Raw, generated Cronet bindings. Set `CRONET_INCLUDE_DIR` and optionally
`CRONET_LIB_DIR` / `CRONET_LIB_NAME`. Prefer the safe `cronet` crate unless
implementing a missing high-level abstraction.

The bundled development ABI contains all 255 native symbols used by
`SagerNet/cronet-go@d62042e`. Native binaries are acquired separately; the
workspace repository provides checksummed Linux and Windows fetch helpers.
