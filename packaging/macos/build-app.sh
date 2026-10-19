#!/bin/sh
# Builds McGyver.app and a .dmg holding it, on a Mac (Xcode's command line tools and Rust).
#
#   packaging/macos/build-app.sh
#
# The app is signed ad hoc, which is enough to run it on the Mac that built it. macOS ties the
# permissions it grants (Input Monitoring) to the signature, and an ad hoc one changes with every
# build: to keep them across builds, make a code signing certificate once (Keychain Access →
# Certificate Assistant → Create a Certificate…, type "Code Signing") and pass its name:
#
#   MCGYVER_SIGN_IDENTITY="McGyver Dev" packaging/macos/build-app.sh
set -eu

cd "$(dirname "$0")/../.."
here=packaging/macos
out=dist
app="$out/McGyver.app"

case "$(uname -m)" in
arm64) target=aarch64-apple-darwin arch=arm64 ;;
x86_64) target=x86_64-apple-darwin arch=x86_64 ;;
*) echo "unknown machine: $(uname -m)" >&2; exit 1 ;;
esac
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)

rustup target add "$target" >/dev/null 2>&1 || true
cargo build --release --locked --target "$target"

rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "${CARGO_TARGET_DIR:-target}/$target/release/mcgyver" "$app/Contents/MacOS/mcgyver"
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

codesign --force --sign "${MCGYVER_SIGN_IDENTITY:--}" --identifier io.github.ferdiunal.mcgyver "$app"
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
