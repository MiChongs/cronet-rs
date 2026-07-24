# Third-party notices

`cronet-rs` is API- and behavior-compatible with
[`SagerNet/cronet-go`](https://github.com/SagerNet/cronet-go), pinned during
development to commit `d62042e935130168f4cebcd4515319a88ee7abcf`.
`cronet-go` is distributed under GPL-3.0-or-later.

The bundled development declarations originate from Chromium Cronet and the
NaiveProxy source tree pinned by that commit. Chromium source files retain
their BSD-style notices in the generated headers.

Native libraries fetched by the helper scripts are unmodified release assets
from SagerNet. They are not included in the crates.io source packages. Their
upstream source, build instructions, license and notices remain available from
the corresponding SagerNet/cronet-go and klzgrad/naiveproxy revisions.
