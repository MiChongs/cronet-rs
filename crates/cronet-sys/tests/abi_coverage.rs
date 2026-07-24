//! ABI coverage checks against the pinned SagerNet Cronet surface.

const CRONET_GO_SYMBOLS: &str = include_str!("../abi/cronet-go-d62042e.symbols");

fn normalized_development_headers() -> String {
    [
        include_str!("../include/cronet_rs_dev.h"),
        include_str!("../include/cronet_rs_naive.h"),
        include_str!("../include/cronet_rs_bidirectional.h"),
    ]
    .concat()
    .split_whitespace()
    .collect()
}

#[test]
fn development_headers_cover_pinned_cronet_go_abi() {
    let headers = normalized_development_headers();
    let missing = CRONET_GO_SYMBOLS
        .lines()
        .filter(|symbol| !headers.contains(&format!("{symbol}(")))
        .collect::<Vec<_>>();

    assert!(
        missing.is_empty(),
        "development headers are missing {} symbols required by \
         SagerNet/cronet-go@d62042e935130168f4cebcd4515319a88ee7abcf:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

#[test]
fn pinned_cronet_go_symbol_manifest_is_stable() {
    assert_eq!(
        CRONET_GO_SYMBOLS.lines().count(),
        255,
        "update the pinned commit and coverage expectation together"
    );
}
