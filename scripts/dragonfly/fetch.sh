#!/usr/bin/env bash
# Fetch and check the tree of one pin of conf/dragonfly/pins.conf, on the host.
#
#   fetch.sh PIN
#
# It prints the path of a tar archive of the tree. A git pin is fetched at depth 1 by its full
# commit hash and written with git archive, so the tree has no .git and nothing in it is generated
# by us. RB_COMMIT, a full commit hash, replaces the pinned commit. When RB_OUT is set, it writes
# "pin kind source hash" to RB_OUT/source.txt. Archives are kept in RB_CACHE.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
pin=$1
: "${RB_CACHE:=$HOME/.cache/rb}"
mkdir -p "$RB_CACHE"

line="$(awk -v p="$pin" '$1 == p' "$here/../../conf/dragonfly/pins.conf")"
if [ -z "$line" ]; then
	echo "no pin $pin in conf/dragonfly/pins.conf" >&2
	exit 2
fi
read -r _ kind source hash <<< "$line"
if [ "$kind" != git ]; then
	echo "pin $pin has an unknown kind $kind" >&2
	exit 2
fi
if [ -n "${RB_COMMIT:-}" ]; then
	if ! [[ $RB_COMMIT =~ ^[0-9a-f]{40}$ ]]; then
		echo "RB_COMMIT must be a full commit hash" >&2
		exit 2
	fi
	hash=$RB_COMMIT
fi
tar_out="$RB_CACHE/dragonfly-src-$pin-${hash:0:12}.tar"
if [ ! -f "$tar_out" ]; then
	repo="$RB_CACHE/git-dragonfly-$pin"
	rm -rf "$repo"
	git init -q "$repo"
	git -C "$repo" fetch -q --depth 1 "$source" "$hash"
	got="$(git -C "$repo" rev-parse FETCH_HEAD)"
	if [ "$got" != "$hash" ]; then
		echo "fetched $got, wanted $hash" >&2
		exit 1
	fi
	git -C "$repo" archive --format=tar -o "$tar_out.part" FETCH_HEAD
	rm -rf "$repo"
	mv "$tar_out.part" "$tar_out"
fi
if [ -n "${RB_OUT:-}" ]; then
	mkdir -p "$RB_OUT"
	printf '%s\t%s\t%s\t%s\n' "$pin" "$kind" "$source" "$hash" > "$RB_OUT/source.txt"
fi
echo "$tar_out"
