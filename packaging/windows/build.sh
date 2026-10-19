#!/bin/sh
# Builds McGyver for Windows, from Linux, with cargo-zigbuild (zig brings MinGW's headers and
# libraries for both machines), and zips each build with its README and licences.
#
#   packaging/windows/build.sh           # x64 and ARM64
#   packaging/windows/build.sh x64       # Intel and AMD only
#   packaging/windows/build.sh arm64     # ARM64 only (Snapdragon and other Windows on ARM)
#
# Needs: cargo-zigbuild and zig, and the Rust targets
#   rustup target add x86_64-pc-windows-gnu aarch64-pc-windows-gnullvm
# The zips land in dist/: McGyver-<version>-windows-<arch>.zip.
set -eu

cd "$(dirname "$0")/../.."
here=packaging/windows
out=dist
target_dir=${CARGO_TARGET_DIR:-target}
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)

case ${1:-all} in
all) builds="x64:x86_64-pc-windows-gnu arm64:aarch64-pc-windows-gnullvm" ;;
x64) builds="x64:x86_64-pc-windows-gnu" ;;
arm64) builds="arm64:aarch64-pc-windows-gnullvm" ;;
*) echo "usage: $0 [all|x64|arm64]" >&2; exit 2 ;;
esac

mkdir -p "$out"
for build in $builds; do
    arch=${build%%:*}
    target=${build#*:}
    # unRAR includes PowrProf.h and Wbemidl.h as the Windows SDK spells them; MinGW's are in
    # lower case, which a case-sensitive file system tells apart.
    env "CXXFLAGS_$(echo "$target" | tr - _)=-I$PWD/$here/include" \
        cargo zigbuild --release --target "$target"

    stage=$(mktemp -d)
    mkdir "$stage/McGyver"
    cp "$target_dir/$target/release/mcgyver.exe" README.md LICENSE-MIT LICENSE-APACHE \
        "$stage/McGyver/"
    zip="$out/McGyver-$version-windows-$arch.zip"
    rm -f "$zip"
    (cd "$stage" && zip -qr - McGyver) >"$zip"
    rm -rf "$stage"
    echo "$zip"
done
