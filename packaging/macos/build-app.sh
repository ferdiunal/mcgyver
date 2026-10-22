#!/bin/sh
# Builds McGyver.app and a .dmg holding it, on a Mac (Xcode's command line tools and Rust from
# rustup, which fetches the other Mac's standard library).
#
#   packaging/macos/build-app.sh             # universal: one app for Apple Silicon and Intel
#   packaging/macos/build-app.sh arm64       # Apple Silicon only, faster to build
#   packaging/macos/build-app.sh x86_64      # Intel only
#
# The app is signed ad hoc, which is enough to run it on the Mac that built it. macOS ties the
# permissions it grants (Input Monitoring) to the signature's designated requirement, which for
# an ad hoc signature is the build's own hash and so changes with every build; here it is the
# bundle identifier instead, so a permission outlives a rebuild (any app claiming that identifier
# would match it too, which is fine on the Mac that builds it). A code signing certificate does
# better (Keychain Access → Certificate Assistant → Create a Certificate…, type "Code Signing"):
#
#   MCGYVER_SIGN_IDENTITY="McGyver Dev" packaging/macos/build-app.sh
set -eu

cd "$(dirname "$0")/../.."
here=packaging/macos
out=dist
app="$out/McGyver.app"

arch=${1:-universal}
case "$arch" in
universal) targets="aarch64-apple-darwin x86_64-apple-darwin" ;;
arm64) targets=aarch64-apple-darwin ;;
x86_64) targets=x86_64-apple-darwin ;;
*) echo "usage: $0 [universal|arm64|x86_64]" >&2; exit 2 ;;
esac
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

# One binary per architecture, then one file holding them all.
set --
for target in $targets; do
	rustup target add "$target" >/dev/null 2>&1 || true
	cargo build --release --locked --target "$target"
	set -- "$@" "${CARGO_TARGET_DIR:-target}/$target/release/mcgyver"
done

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
# Cargo's binary is mcgyver, as Linux has it; in the bundle it goes by the app's name.
lipo -create -output "$app/Contents/MacOS/McGyver" "$@"
lipo -info "$app/Contents/MacOS/McGyver"
sed "s/@VERSION@/$version/g" "$here/Info.plist" >"$app/Contents/Info.plist"
printf 'APPL????' >"$app/Contents/PkgInfo"

# The icon at every size Finder and the Dock ask for.
iconset=$(mktemp -d)/AppIcon.iconset
mkdir -p "$iconset"
for size in 16 32 128 256 512; do
	sips -z "$size" "$size" "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}.png" >/dev/null
	double=$((size * 2))
	sips -z "$double" "$double" "$here/icon-1024.png" --out "$iconset/icon_${size}x${size}@2x.png" >/dev/null
done
iconutil -c icns "$iconset" -o "$app/Contents/Resources/AppIcon.icns"
rm -rf "$(dirname "$iconset")"

id=io.github.ferdiunal.mcgyver
if [ -n "${MCGYVER_SIGN_IDENTITY:-}" ]; then
	codesign --force --sign "$MCGYVER_SIGN_IDENTITY" --identifier "$id" "$app"
else
	codesign --force --sign - --identifier "$id" --requirements "=designated => identifier \"$id\"" "$app"
fi
codesign --verify --strict "$app"

# The disk image: the app next to a link to Applications, to drag it onto.
dmg="$out/McGyver-$version-$arch.dmg"
stage=$(mktemp -d)
cp -R "$app" "$stage/"
ln -s /Applications "$stage/Applications"
rm -f "$dmg"
hdiutil create -quiet -volname McGyver -srcfolder "$stage" -ov -format UDZO "$dmg"
rm -rf "$stage"

echo "$app"
echo "$dmg"
