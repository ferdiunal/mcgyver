#!/bin/sh
# Builds McGyver for Windows, from Linux, with cargo-zigbuild (zig brings MinGW's headers and
# libraries for both machines), and zips each build, as McGyver.exe with its icon and version
# details (McGyver.rc, compiled by zig rc), with its README and licences.
#
#   packaging/windows/build.sh           # x64 and ARM64
#   packaging/windows/build.sh x64       # Intel and AMD only
#   packaging/windows/build.sh arm64     # ARM64 only (Snapdragon and other Windows on ARM)
#
# Needs: cargo-zigbuild and zig, and the Rust targets
#   rustup target add x86_64-pc-windows-gnu aarch64-pc-windows-gnullvm
# and curl and 7z for Mesa's software OpenGL, which the x64 zip carries in McGyver/opengl.
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
# The resource: the icon, and the version as Explorer shows it (1.2.3, and 1,2,3 for the numbers).
res_dir=$(mktemp -d)
trap 'rm -rf "$res_dir"' EXIT
trap 'exit 130' INT TERM
numbers=$(echo "${version%%[-+]*}" | tr . ,)
sed -e "s|@VERSION@|$version|g" -e "s|@VERSION_COMMAS@|$numbers|g" \
    "$here/McGyver.rc" >"$res_dir/McGyver.rc"
# The icon is found by /i, beside McGyver.rc: no path to quote.
zig rc /i "$PWD/$here" /fo "$res_dir/McGyver.res" -- "$res_dir/McGyver.rc"

# Mesa's software OpenGL (llvmpipe), for where OpenGL is older than 2.0 (src/software_gl.rs):
# pal1000/mesa-dist-win's build, MIT-licensed, checked against the hash it had when it was
# chosen and kept in the target folder. It has no ARM64 build; Windows on ARM has Microsoft's
# OpenGL compatibility pack instead.
mesa_version=26.2.4
mesa_sha256=351fc8c8b695878ffb3eaa044b3ead08672a48b1a045e3c3e3975811df0f6695
mesa_dir="$target_dir/mesa-$mesa_version"
mesa() {
    if [ ! -f "$mesa_dir/x64/libgallium_wgl.dll" ]; then
        archive="$mesa_dir.7z"
        curl -fsSL -o "$archive" "https://github.com/pal1000/mesa-dist-win/releases/download/$mesa_version/mesa3d-$mesa_version-release-msvc.7z"
        echo "$mesa_sha256  $archive" | sha256sum -c --quiet
        7z x -y -bso0 -o"$mesa_dir" "$archive" x64/opengl32.dll x64/libgallium_wgl.dll
        rm -f "$archive"
    fi
    mkdir "$1"
    cp "$mesa_dir/x64/opengl32.dll" "$mesa_dir/x64/libgallium_wgl.dll" "$here/opengl-README.txt" "$1/"
    mv "$1/opengl-README.txt" "$1/README.txt"
}

for build in $builds; do
    arch=${build%%:*}
    target=${build#*:}
    # unRAR includes PowrProf.h and Wbemidl.h as the Windows SDK spells them; MinGW's are in
    # lower case, which a case-sensitive file system tells apart.
    # rustc rather than build: the resource goes to the final link alone, so the dependencies
    # keep their build.
    env "CXXFLAGS_$(echo "$target" | tr - _)=-I$PWD/$here/include" \
        cargo-zigbuild rustc --release --target "$target" --bin mcgyver \
        -- -C "link-arg=$res_dir/McGyver.res"

    stage=$(mktemp -d)
    mkdir "$stage/McGyver"
    cp "$target_dir/$target/release/mcgyver.exe" "$stage/McGyver/McGyver.exe"
    cp README.md LICENSE-MIT LICENSE-APACHE "$stage/McGyver/"
    if [ "$arch" = x64 ]; then
        mesa "$stage/McGyver/opengl"
    fi
    zip="$out/McGyver-$version-windows-$arch.zip"
    rm -f "$zip"
    (cd "$stage" && zip -qr - McGyver) >"$zip"
    rm -rf "$stage"
    echo "$zip"
done
