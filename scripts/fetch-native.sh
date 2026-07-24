#!/usr/bin/env sh
set -eu

release="v148.0.7778.96-1"
architecture="${1:-}"
destination="${2:-target/cronet-sdk}"

if [ -z "$architecture" ]; then
    case "$(uname -m)" in
        x86_64) architecture="amd64" ;;
        i386|i486|i586|i686) architecture="386" ;;
        aarch64|arm64) architecture="arm64" ;;
        armv7l|armv6l) architecture="arm" ;;
        loongarch64) architecture="loong64" ;;
        mips64el) architecture="mips64le" ;;
        mipsel) architecture="mipsle" ;;
        riscv64) architecture="riscv64" ;;
        *) echo "Unsupported Linux architecture: $(uname -m)" >&2; exit 1 ;;
    esac
fi

root="$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)"
asset="libcronet-linux-$architecture.so"
expected="$(awk -v asset="$asset" '$2 == asset { print $1 }' "$root/native/SHA256SUMS")"
if [ -z "$expected" ]; then
    echo "No checksum is pinned for $asset" >&2
    exit 1
fi

case "$destination" in
    /*) sdk="$destination" ;;
    *) sdk="$root/$destination" ;;
esac
mkdir -p "$sdk/lib"
download="$sdk/lib/$asset"
url="https://github.com/SagerNet/cronet-go/releases/download/$release/$asset"
curl --fail --location --retry 3 --output "$download" "$url"
actual="$(sha256sum "$download" | awk '{ print $1 }')"
if [ "$actual" != "$expected" ]; then
    rm -f "$download"
    echo "SHA-256 mismatch for $asset: expected $expected, received $actual" >&2
    exit 1
fi

exports="$sdk/lib/.cronet-exports"
nm -D --defined-only "$download" | awk '{ print $3 }' | sort -u > "$exports"
missing=0
while IFS= read -r symbol; do
    case "$symbol" in
        ""|\#*) continue ;;
    esac
    if ! grep -Fqx "$symbol" "$exports"; then
        echo "Native library is missing required export: $symbol" >&2
        missing=1
    fi
done < "$root/crates/cronet-sys/abi/cronet-go-d62042e.symbols"
rm -f "$exports"
if [ "$missing" -ne 0 ]; then
    exit 1
fi

cp "$download" "$sdk/lib/libcronet.so"
printf '%s\n' \
    "Verified $asset ($actual)" \
    "CRONET_LIB_DIR=$sdk/lib" \
    "CRONET_LIB_NAME=cronet" \
    "Add to LD_LIBRARY_PATH: $sdk/lib"
