#!/bin/sh
# Puts a release on GitHub from this machine: tags the version in Cargo.toml, builds what this
# machine can, and uploads it to that version's release, a draft until --publish. Linux builds
# the .deb and the .rpm (packaging/linux/build.sh) and the Windows zips (packaging/windows/
# build.sh); a Mac builds the .dmg (packaging/macos/build-app.sh). So: run it on Linux, then on a
# Mac from the same commit, then --publish from either.
#
#   packaging/release.sh             # build and upload to the draft
#   packaging/release.sh --publish   # the draft becomes the latest release: the site's downloads
#                                    # and the apps' update check follow within minutes
#   packaging/release.sh --force     # build and upload again to a release already published
#
# Needs gh, logged in to an account that can write to the repo, and the release notes in
# packaging/notes/<version>.md.
set -eu

cd "$(dirname "$0")/.."
version=$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
if [ -z "$version" ]; then
	echo "no version in Cargo.toml" >&2
	exit 1
fi
tag="v$version"

if [ "${1:-}" = --publish ]; then
	# Every download button needs its file: one missing would land on the page again.
	names=$(gh release view "$tag" --json assets --jq '.assets[].name')
	for end in _amd64.deb .x86_64.rpm -universal.dmg -windows-x64.zip -windows-arm64.zip; do
		if ! printf '%s\n' "$names" | grep -q -- "$end\$"; then
			echo "$tag has no *$end yet" >&2
			exit 1
		fi
	done
	gh release edit "$tag" --draft=false --latest
	gh release view "$tag" --json url,assets --jq '.url, .assets[].name'
	exit 0
fi

# A release is a commit: what isn't committed, new files included, doesn't go in.
if [ -n "$(git status --porcelain -- src Cargo.toml Cargo.lock packaging assets vendor)" ]; then
	echo "commit or stash the changes first" >&2
	exit 1
fi
git fetch --quiet --tags origin
if git rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
	if [ "$(git rev-parse "$tag^{commit}")" != "$(git rev-parse HEAD)" ]; then
		echo "$tag is another commit: git checkout $tag first" >&2
		exit 1
	fi
else
	git tag -a "$tag" -m "McGyver $version"
fi
git push --quiet origin "$tag"

if ! gh release view "$tag" >/dev/null 2>&1; then
	notes="packaging/notes/$version.md"
	if [ ! -f "$notes" ]; then
		echo "write $notes first" >&2
		exit 1
	fi
	gh release create "$tag" --draft --verify-tag --title "McGyver $version" --notes-file "$notes"
elif [ "$(gh release view "$tag" --json isDraft --jq .isDraft)" != true ] && [ "${1:-}" != --force ]; then
	# Files people already download are replaced only on purpose.
	echo "$tag is published; pass --force to replace its files" >&2
	exit 1
fi

case "$(uname -s)" in
Linux)
	packaging/linux/build.sh
	packaging/windows/build.sh
	files="dist/mcgyver_$version-*_amd64.deb dist/mcgyver-$version-*.x86_64.rpm
		dist/McGyver-$version-windows-x64.zip dist/McGyver-$version-windows-arm64.zip"
	;;
Darwin)
	packaging/macos/build-app.sh
	files="dist/McGyver-$version-universal.dmg"
	;;
*)
	echo "a release is built on Linux and on a Mac" >&2
	exit 1
	;;
esac
# Unquoted on purpose: the names hold the packages' revisions as patterns.
# shellcheck disable=SC2086
gh release upload "$tag" $files --clobber
gh release view "$tag" --json assets --jq '.assets[].name'
