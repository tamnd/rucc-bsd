#!/bin/sh
# Make the FreeBSD package of rucc (plan 16.11, B10), run as root in the builder VM after
# rb-tools.sh, which installs rustup in /build/rust.
#
#   rucc-pkg.sh OUTDIR [COMMIT]
#
# COMMIT is a full commit of tamnd/rucc. With no COMMIT, it is the commit of the [rucc] pin in
# toolchains.toml of the harness. cargo builds rucc from the tarball of that commit with --locked,
# so the build takes the versions of its Cargo.lock and nothing else.
#
# The package is rucc-VERSION.pkg in /build/pkg. VERSION is the version that rucc --version prints,
# then .g and the first 9 digits of the commit, such as 0.30.15.gc98a18353. The package has
# /usr/local/bin/rucc, the link /usr/local/bin/cc to it, and /usr/local/bin/rucc.source. That file
# names the repo, the commit and the SHA-256 of the binary, as scripts/netbsd/rucc.sh writes it, so
# rb takes the binary of the package as the pinned rucc. The annotations of the package give the
# commit and the SHA-256 too. rucc links with ld on PATH, so the package depends on binutils.
# The package goes into the base system of no image. pkg add installs it in a running system.
# OUTDIR/rucc-pkg.txt records the commit, the versions and the SHA-256 of the binary and the package.
set -eu
out=$1
commit=${2:-}
: "${RB_HARNESS:=/root/harness}"
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
export RUSTUP_HOME=/build/rust/rustup CARGO_HOME=/build/rust/cargo
export PATH="$CARGO_HOME/bin:$PATH" CARGO_BUILD_JOBS="$RB_JOBS"
repo=tamnd/rucc
if [ -z "$commit" ]; then
	commit=$(sed -n '/^\[rucc\]/,/^\[/s/^commit = "\([0-9a-f]*\)"$/\1/p' "$RB_HARNESS/toolchains.toml")
fi
case $commit in
*[!0-9a-f]* | "")
	echo "rucc-pkg.sh: give a full commit of $repo, or pin one in toolchains.toml" >&2
	exit 2
	;;
esac
if [ "${#commit}" -ne 40 ]; then
	echo "rucc-pkg.sh: $commit is not a full commit" >&2
	exit 2
fi
if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
	echo "rucc-pkg.sh: no cargo in $CARGO_HOME; run rb-tools.sh first" >&2
	exit 2
fi
mkdir -p "$out" /build/pkg
src=/build/rucc-pkg/src
stage=/build/rucc-pkg/stage
rm -rf /build/rucc-pkg
mkdir -p "$src" "$stage/usr/local/bin"
start=$(date +%s)
fetch -q -o - "https://codeload.github.com/$repo/tar.gz/$commit" | tar -xzf - -C "$src" --strip-components 1
(cd "$src" && rustup toolchain install && CARGO_TARGET_DIR="$src/target" cargo build -q --release --locked -p rucc)
bin=$stage/usr/local/bin
install -m 555 "$src/target/release/rucc" "$bin/rucc"
ln -s rucc "$bin/cc"
digest=$(sha256 -q "$bin/rucc")
printf 'repo = "%s"\ncommit = "%s"\nsha256 = "%s"\n' "$repo" "$commit" "$digest" > "$bin/rucc.source"
chmod 444 "$bin/rucc.source"
version=$("$bin/rucc" --version | awk 'NR == 1 { print $2 }')
case $version in
[0-9]*.[0-9]*) ;;
*)
	echo "rucc-pkg.sh: rucc --version gives no version: $("$bin/rucc" --version | head -1)" >&2
	exit 1
	;;
esac
pkgversion=$version.g$(echo "$commit" | cut -c1-9)
binutils=$(pkg query '%v' binutils 2>/dev/null || pkg rquery '%v' binutils)

cat > /build/rucc-pkg/manifest <<EOF
name: rucc
version: "$pkgversion"
origin: devel/rucc
comment: "An optimizing C compiler written in Rust"
www: "https://github.com/$repo"
maintainer: "1218621+tamnd@users.noreply.github.com"
prefix: /usr/local
licenselogic: single
licenses: [APACHE20]
categories: [devel, lang]
desc: "rucc $version, built from $repo commit $commit. It links with ld of binutils."
deps: {
	binutils: { origin: devel/binutils, version: "$binutils" }
}
annotations: {
	rucc_commit: "$commit",
	rucc_sha256: "$digest"
}
EOF
cat > /build/rucc-pkg/plist <<'EOF'
bin/rucc
bin/rucc.source
bin/cc
EOF
rm -f /build/pkg/rucc-*.pkg
SOURCE_DATE_EPOCH=$(stat -f %m "$src/Cargo.toml") \
	pkg create -M /build/rucc-pkg/manifest -p /build/rucc-pkg/plist -r "$stage" -o /build/pkg
file=/build/pkg/rucc-$pkgversion.pkg
{
	echo "# the rucc package"
	echo "repo $repo"
	echo "commit $commit"
	echo "version $version"
	echo "package rucc-$pkgversion"
	echo "binary_sha256 $digest"
	echo "package_sha256 $(sha256 -q "$file")"
	echo "package_bytes $(stat -f %z "$file")"
	echo "binutils $binutils"
	echo "seconds $(($(date +%s) - start))"
	echo
	echo "# rucc --version"
	"$bin/rucc" --version 2>&1 | head -3
	echo
	echo "# pkg info -F $file"
	pkg info -F "$file"
	echo
	echo "# pkg info -l -F $file"
	pkg info -l -F "$file"
} > "$out/rucc-pkg.txt"
cat "$out/rucc-pkg.txt"
