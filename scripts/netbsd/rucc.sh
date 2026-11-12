#!/usr/bin/env bash
# Put rucc on PATH for a job of netbsd-r.yml, and write in the step summary which rucc it is.
#
# REF, the rucc input of the run, names a branch or commit of tamnd/rucc. It is built from
# source. rb build calls such a build ungraded, unless REF is the pinned commit. With no REF, the
# [rucc] pin of toolchains.toml decides. A commit pin is built from source at that commit. A
# release pin is the release tarball, checked against [rucc.tarball_sha256].
#
# A build from source writes rucc.source beside the binary, with the repo, the commit that the
# checkout gives and the SHA-256 of the binary. rb takes the binary as the pinned rucc only when
# that file names the pinned repo and commit and the SHA-256 of the binary.
#
# "rucc.sh name" prints the rucc of the run: REF, else the pinned commit, else the release tag.
set -euo pipefail

host=x86_64-unknown-linux-gnu
summary=${GITHUB_STEP_SUMMARY:-/dev/stderr}
temp=${RUNNER_TEMP:-/tmp}
ref=${REF:-}

# The [rucc] pin as "KEY=VALUE" lines.
pin() {
  python3 - "$host" <<'EOF'
import sys, tomllib
with open("toolchains.toml", "rb") as f:
    rucc = tomllib.load(f)["rucc"]
print(f"repo={rucc['repo']}")
print(f"commit={rucc.get('commit', '')}")
print(f"version={rucc.get('version', '')}")
print(f"tarball_sha256={rucc.get('tarball_sha256', {}).get(sys.argv[1], '')}")
EOF
}

repo='' commit='' version='' tarball_sha256=''
while IFS='=' read -r key value; do
  case $key in
  repo) repo=$value ;;
  commit) commit=$value ;;
  version) version=$value ;;
  tarball_sha256) tarball_sha256=$value ;;
  esac
done < <(pin)

if [ "${1:-}" = name ]; then
  echo "${ref:-${commit:-$version}}"
  exit 0
fi

# Build rucc from source at $1, put it on PATH and write rucc.source beside it.
from_source() {
  local src="$temp/rucc-src" bin head digest locked=()
  git clone -q "https://github.com/$repo" "$src"
  git -C "$src" checkout -q "$1"
  head=$(git -C "$src" rev-parse HEAD)
  # A pinned commit builds with the versions of its Cargo.lock and nothing else.
  if [ -z "$ref" ]; then
    locked=(--locked)
  fi
  cargo build --release "${locked[@]}" --manifest-path "$src/Cargo.toml" -p rucc
  bin="$src/target/release/rucc"
  digest=$(sha256sum "$bin" | cut -d' ' -f1)
  printf 'repo = "%s"\ncommit = "%s"\nsha256 = "%s"\n' "$repo" "$head" "$digest" > "$bin.source"
  echo "$src/target/release" >> "${GITHUB_PATH:-/dev/null}"
  echo "rucc built from $repo commit $head, $2. The binary has sha256 $digest." >> "$summary"
}

if [ -n "$ref" ]; then
  from_source "$ref" "the rucc input $ref, not the pin"
elif [ -n "$commit" ]; then
  from_source "$commit" "the pinned commit of toolchains.toml"
else
  name="rucc-$version-$host"
  if [ -z "$tarball_sha256" ]; then
    echo "rucc.sh: toolchains.toml gives no tarball_sha256 for $host" >&2
    exit 1
  fi
  gh release download "$version" -R "$repo" -p "$name.tar.gz" -D "$temp"
  echo "$tarball_sha256  $temp/$name.tar.gz" | sha256sum -c -
  tar -xzf "$temp/$name.tar.gz" -C "$temp"
  echo "$temp/$name" >> "${GITHUB_PATH:-/dev/null}"
  digest=$(sha256sum "$temp/$name/rucc" | cut -d' ' -f1)
  echo "rucc $version, the release tarball of toolchains.toml. The binary has sha256 $digest." >> "$summary"
fi
