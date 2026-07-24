# cronet-sys

Raw, generated Cronet bindings. The default build uses checked-in declarations
for the pinned SagerNet ABI and does not require libclang. To bind another SDK,
enable feature `generate-bindings`, install libclang, set `CRONET_INCLUDE_DIR`
and optionally set `CRONET_LIB_DIR` / `CRONET_LIB_NAME`. Prefer the safe
`cronet` crate unless implementing a missing high-level abstraction.

The bundled development ABI contains all 255 native symbols used by
`SagerNet/cronet-go@d62042e`. Native binaries are acquired separately; the
workspace repository provides checksummed Linux and Windows fetch helpers.
