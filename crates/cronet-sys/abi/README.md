# ABI baselines

`cronet-go-d62042e.symbols` is the sorted native symbol set loaded by:

- `SagerNet/cronet-go`
- commit `d62042e935130168f4cebcd4515319a88ee7abcf`
- `internal/cronet/loader_unix.go`
- `internal/cronet/loader_windows.go`

The integration test requires the bundled development declarations to cover
this entire set. Updating the upstream baseline requires reviewing the symbol
diff and changing the pinned commit and expected count in the same change.
