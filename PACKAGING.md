# Packaging

The product is McGyver: snap and package `mcgyver`, binary `mcgyver`, bus name `io.github.McGyver`.
The name lives in `snap/snapcraft.yaml`, `packaging/linux/mcgyver.desktop`, the package name and asset
paths in `Cargo.toml`, `APP_ID`/`APP_NAME` (`src/settings.rs`), the bus name (`src/service.rs`) and
the KGlobalAccel component (`src/tap.rs`). Installs from when it was called Zero-Click are taken over:
its KWin entry is replaced and its Shift shortcut released on the first `--install` or start, its
settings are copied when McGyver has none yet, and its start-at-login entry is replaced.

## What is here

| File | Purpose |
|---|---|
| `snap/snapcraft.yaml` | Snap Store package (classic confinement, core24) |
| `packaging/linux/mcgyver.desktop` | Menu entry, shared by all packages |
| `Cargo.toml` | `[package.metadata.deb]` and `[package.metadata.generate-rpm]`, read by cargo-deb and cargo-generate-rpm |
| `packaging/release.sh` | Tags the version, builds what the machine can and uploads it to a draft GitHub release; `--publish` makes it the latest (see [Releases](#releases)) |
| `packaging/linux/build.sh` | The .deb in an Ubuntu 24.04 container and the .rpm in a Fedora 43 one (podman) |
| `packaging/notes/<version>.md` | Each release's notes |
| `.github/workflows/release.yml` | Run by hand only: builds snap, .deb, .rpm as artifacts and, on a tag, publishes the snap to `edge` if the secret exists; it never touches the GitHub release |

## Why classic confinement

The app makes arbitrary session D-Bus calls to KDE services (kglobalaccel, Dolphin, StatusNotifierWatcher),
uses KWin-only Wayland protocols (layer-shell, keystate), converts files anywhere (home, `/media`, hidden
dirs) and launches host tools (terminal, kdialog, wl-copy, kdeconnect-cli, pw-play). Strict interfaces
cannot express this, so the snap is `confinement: classic`. Classic snaps need a manual store review.
Consequence: host tools (kdialog, wl-copy, pw-play) come from the user's system, not the snap.

## Releases

Releases are built on our own machines, not in GitHub Actions:

1. Set `version` in `Cargo.toml` (the site's version lines in `site/src` too), write
   `packaging/notes/<version>.md`, commit and push.
2. On Linux: `packaging/release.sh`. It tags `v<version>` on that commit and pushes the tag,
   opens a draft release with the notes, builds the .deb and the .rpm in containers
   (`packaging/linux/build.sh`) and the Windows zips (`packaging/windows/build.sh`), and uploads
   them. Run again, it rebuilds and replaces them.
3. On a Mac, from the same commit (`git fetch --tags && git checkout v<version>`):
   `packaging/release.sh` builds the universal .dmg and uploads it to the same draft.
4. `packaging/release.sh --publish`: the draft becomes the latest release, once it holds all five
   files (.deb, .rpm, .dmg and both zips).
5. Deploy the site after publishing (`cd site && npm run deploy`), with its version lines updated:
   its download buttons hand out the latest published release, so they work only from then on.

A published release's files are replaced only with `packaging/release.sh --force`.

The repository is private, so the files aren't public on GitHub: the site hands them out.
`https://mcgyver.ferdiunal.tr/download/<deb|rpm|macos|windows-x64|windows-arm64>` finds the file in
the latest release and redirects to a short-lived link to it, with a read-only token
(`GITHUB_TOKEN`, a secret of the site's Worker; see `site/wrangler.jsonc`). The apps' daily update
check asks the same Worker (`/api/ping`, `/api/latest`), which reads the latest release's tag, so
publishing is all an update takes. A draft or a pre-release is never offered.

## Build locally

- **.deb and .rpm for a release:** `packaging/linux/build.sh` (podman, rootless): built in Ubuntu
  24.04 and Fedora 43, so they ask for no newer glibc than those have → `dist/`.
- **.deb** (Debian/Ubuntu, best on Ubuntu 24.04 so the glibc requirement stays low; a build on Fedora 44
  needs glibc 2.43 and will not run on Debian 12/Ubuntu 24.04):
  `cargo install cargo-deb && cargo build --release && cargo deb --no-build`
  -> `target/debian/*.deb`. (`$auto` dependency detection does not work on Fedora hosts.)
- **.rpm** (Fedora): `cargo install cargo-generate-rpm && cargo build --release && cargo generate-rpm`
  -> `target/generate-rpm/*.rpm`
- **snap**: `sudo snap install snapcraft --classic`, plus LXD (`sudo snap install lxd && sudo lxd init --auto`)
  or Multipass, then `snapcraft` in the repo root. Build from a clean checkout: `source: .` copies the
  working tree, including a huge `target/`. On Fedora the CI route (`snap` job) is easier.
  Test: `sudo snap install --dangerous --classic ./mcgyver_*.snap`.

## One-time Snap Store steps (you must do these)

1. Create an Ubuntu One account and log in at snapcraft.io.
2. `snapcraft login`, then `snapcraft register mcgyver` (the name may be taken; pick the final name first).
3. Request classic confinement: post in the "classic-confinement" category on https://forum.snapcraft.io
   (title: `Classic confinement request: mcgyver`). Draft:

   > Name: mcgyver. Summary: a radial drop overlay for KDE Plasma 6 (Wayland) that converts
   > archives, images and documents. Why strict does not work: (1) it calls arbitrary methods on session
   > D-Bus services owned by KDE (org.kde.kglobalaccel, org.kde.dolphin-*, StatusNotifierWatcher), which no
   > interface allows; (2) it uses KWin-specific Wayland protocols (layer-shell overlay, org_kde_kwin_keystate);
   > (3) it must read and write the user's files anywhere (home, /media, hidden directories), where `home`
   > and `removable-media` are insufficient; (4) it launches host programs (terminal, kdialog, wl-copy,
   > kdeconnect-cli, pw-play). Upstream: <repo URL>. Licence: MIT OR Apache-2.0.

   Reviewers vote; this can take days to weeks. Until approved, uploads work but cannot be released to stable.
4. CI credentials: `snapcraft export-login --snaps mcgyver --acls package_access,package_push,package_release -`
   and store the output as the GitHub secret `SNAPCRAFT_STORE_CREDENTIALS`. The publish job is skipped without it.
5. Run the release workflow by hand on the tag (Actions → release → Run workflow, `vX.Y.Z`): it
   builds and pushes to `edge`. Promote with `snapcraft release mcgyver <rev> stable`
   (needs `grade: stable` in `snapcraft.yaml`).

Fedora users need snapd for classic snaps: `sudo dnf install snapd && sudo ln -s /var/lib/snapd/snap /snap`
(log out/in or reboot once). Debian: `sudo apt install snapd`.

## Autostart

The settings window's "Start at login" writes `~/.config/autostart/mcgyver.desktop`. Its
`Exec` is `/snap/bin/mcgyver` inside the snap (the binary's own path changes with every
revision), the AppImage when run from one, else the binary. The snap has no `autostart:` key: snapd
only reads `$SNAP_USER_DATA/.config/autostart` for it, and a classic snap writes the real one.

## Snapcraft docs relied on

- Classic confinement (RPATH, `$ORIGIN`, interpreter, PatchELF; core24 looks in `/snap/core24` when a lib is
  missing from the snap): https://ubuntu.com/docs/snapcraft/en/stable/explanation/classic-confinement/
- Rust plugin: `rust-inherit-ldflags` and the core24 classic `LDFLAGS` example:
  https://ubuntu.com/docs/snapcraft/9/common/craft-parts/reference/plugins/rust_plugin/
- `build-attributes: enable-patchelf` (opt-in for core22+ classic), `autostart`, `desktop`:
  https://ubuntu.com/docs/snapcraft/stable/reference/snapcraft-yaml/ and
  https://forum.snapcraft.io/t/how-to-set-up-classic-confinement/34416

## macOS

`packaging/macos/build-app.sh`, run on a Mac, builds `dist/McGyver.app` (`Info.plist` from
`packaging/macos/`, `LSUIElement` so it lives in the menu bar; bundle id
`io.github.ferdiunal.mcgyver`, also `BUNDLE_ID` in `src/settings.rs`; the icon made from
`packaging/macos/icon-1024.png` with `sips` and `iconutil`), signs it, and wraps it in
`dist/McGyver-<version>-<arch>.dmg`. The arch is `universal` unless `arm64` or `x86_64` is
passed: both are built (`aarch64-apple-darwin`, `x86_64-apple-darwin`) and joined with `lipo`.
The signature is ad hoc unless `MCGYVER_SIGN_IDENTITY` names a certificate in the keychain; see
the README. It can't be built from Linux: linking needs Apple's SDK. From Linux,
`cargo check --target aarch64-apple-darwin` (and `x86_64-apple-darwin`, with
`CC_<target>` pointing at a `zig cc -target aarch64-macos` or `x86_64-macos` wrapper for the C
in zstd and friends) checks that it compiles. Distribution beyond the build machine needs a Developer ID certificate and notarization
(`xcrun notarytool`); Homebrew would come after.

## Windows

`packaging/windows/build.sh`, run on Linux, builds the app for `x86_64-pc-windows-gnu` and
`aarch64-pc-windows-gnullvm` with `cargo zigbuild` and zips each, as `McGyver.exe`, with the
README and licences: `dist/McGyver-<version>-windows-x64.zip` and `-arm64.zip` (pass `x64` or
`arm64` for one). `McGyver.rc` gives the exe its icon (`McGyver.ico`) and the version details
Explorer and Task Manager show: `zig rc` compiles it, and `cargo-zigbuild rustc` links it into
the exe alone. The x64 zip also carries Mesa's software OpenGL in `McGyver/opengl`
(`opengl32.dll` and `libgallium_wgl.dll` from pal1000/mesa-dist-win, MIT, with
`opengl-README.txt`), for machines with only Windows' OpenGL 1.1 (`src/software_gl.rs`): the
script downloads the pinned release once (curl, checked by SHA-256), unpacks the two files with
7z and keeps them in the target folder.
unRAR's sources include `PowrProf.h` and `Wbemidl.h` as the Windows SDK spells them; MinGW's are
in lower case, so `packaging/windows/include` forwards them, through `CXXFLAGS_<target>`. Its
build script chose its Windows sources by the build machine, so `vendor/unrar_sys` asks for the
target instead. On Windows itself, `cargo build --release` (MSVC) needs neither.

The exe is unsigned, so SmartScreen warns on first start; a code signing certificate (or Azure
Trusted Signing) would stop that. It is handed out through the site from GitHub Releases (see
[Releases](#releases)); winget would come after.
