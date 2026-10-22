#!/bin/sh
# Builds the Linux packages in containers of the oldest systems they are for, so the binary asks
# for no newer glibc than those have: the .deb in Ubuntu 24.04, the .rpm in Fedora 43. Needs
# podman (rootless is fine). Rust, cargo-deb and the crates are kept in volumes between runs; each
# system builds into a target folder of its own. (git fetches the git dependencies: see
# .cargo/config.toml.)
#
#   packaging/linux/build.sh          # both → dist/
#   packaging/linux/build.sh deb      # or rpm: one of them
set -eu

cd "$(dirname "$0")/../.."
mkdir -p dist

# build <image> <target folder> <build dependencies, installed as root> <what to build>
build() {
	podman run --rm --security-opt label=disable \
		-v "$PWD:/src" -w /src \
		-v mcgyver-cargo:/root/.cargo -v mcgyver-rustup:/root/.rustup \
		-e CARGO_TARGET_DIR="/src/target/$2" \
		"$1" sh -euc "$3
			if [ ! -x /root/.cargo/bin/cargo ]; then
				curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --no-modify-path
			fi
			export PATH=/root/.cargo/bin:\$PATH
			rustup update stable --no-self-update >/dev/null 2>&1 || true
			$4"
}

for kind in ${1:-deb rpm}; do
	case "$kind" in
	deb)
		# Reads [package.metadata.deb] from Cargo.toml.
		build docker.io/library/ubuntu:24.04 ubuntu-24.04 \
			"apt-get update -qq
			DEBIAN_FRONTEND=noninteractive apt-get install -y -qq build-essential pkg-config \
				libwayland-dev libxkbcommon-dev curl ca-certificates git >/dev/null" \
			"command -v cargo-deb >/dev/null || cargo install cargo-deb --locked
			cargo deb --locked --output dist/"
		;;
	rpm)
		# Reads [package.metadata.generate-rpm] from Cargo.toml.
		build registry.fedoraproject.org/fedora:43 fedora-43 \
			"dnf install -y -q gcc gcc-c++ make cmake clang pkgconf-pkg-config wayland-devel \
				libxkbcommon-devel curl git >/dev/null" \
			"command -v cargo-generate-rpm >/dev/null || cargo install cargo-generate-rpm --locked
			cargo build --release --locked
			cargo generate-rpm --output dist/"
		;;
	*)
		echo "usage: $0 [deb|rpm]" >&2
		exit 2
		;;
	esac
done
ls -l dist/*.deb dist/*.rpm 2>/dev/null || true
