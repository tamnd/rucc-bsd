#!/bin/sh
# Build rb and the shims in the builder VM, and set up the compilers of one side. Run as root in
# the VM after provision.sh.
#
#   rb-tools.sh OUTDIR ref | rucc [RUCC_COMMIT]
#
# rustup installs the Rust of rust-toolchain.toml under /build/rust, from the rustup-init that
# static.rust-lang.org publishes, checked by its SHA-256. cargo builds rb and rb-cc from the
# harness copy in RB_HARNESS. For rucc, it also builds rucc from the tarball of RUCC_COMMIT of
# tamnd/rucc. The binaries go to /build/rb/bin.
#
# Then rb toolchain RB_PIN_NAME writes /build/toolchain: the shims in bin, and ref.mk and rucc.mk.
# The shims write one record per compiler call to RB_COMPILE_JSONL. A rucc that is not the pinned
# release makes the build ungraded. OUTDIR/rb-tools.txt records the versions and the checks.
set -eu
out=$1
side=$2
commit=${3:-}
: "${RB_HARNESS:=/root/harness}"
: "${RB_PIN_NAME:=freebsd-15.1}"
: "${RB_COMPILE_JSONL:=/build/out/compile.jsonl}"
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
export RUSTUP_HOME=/build/rust/rustup CARGO_HOME=/build/rust/cargo
export PATH="$CARGO_HOME/bin:$PATH" CARGO_BUILD_JOBS="$RB_JOBS"
bin=/build/rb/bin
mkdir -p "$out" "$bin" /build/rust "$(dirname "$RB_COMPILE_JSONL")"
start=$(date +%s)

if [ ! -x "$CARGO_HOME/bin/cargo" ]; then
	url=https://static.rust-lang.org/rustup/dist/x86_64-unknown-freebsd/rustup-init
	fetch -q -o /build/rust/rustup-init "$url"
	want=$(fetch -q -o - "$url.sha256" | awk '{ print $1 }')
	got=$(sha256 -q /build/rust/rustup-init)
	if [ "$want" != "$got" ]; then
		echo "rustup-init has SHA-256 $got, $url.sha256 gives $want" >&2
		exit 1
	fi
	chmod +x /build/rust/rustup-init
	/build/rust/rustup-init -y -q --no-modify-path --profile minimal --default-toolchain none
fi

cd "$RB_HARNESS"
# With no name, rustup installs the toolchain that rust-toolchain.toml in the directory names.
rustup toolchain install
CARGO_TARGET_DIR=/build/rb/target cargo build -q --release --locked -p rb --bin rb -p rb-shim --bin rb-cc
cp /build/rb/target/release/rb /build/rb/target/release/rb-cc "$bin/"
{
	echo "# rust"
	rustc --version
	cargo --version
} > "$out/rb-tools.txt"

rucc_flags=
if [ "$side" = rucc ]; then
	if [ -z "$commit" ]; then
		echo "give the commit of tamnd/rucc to build" >&2
		exit 2
	fi
	rm -rf /build/rucc
	mkdir -p /build/rucc
	fetch -q -o - "https://codeload.github.com/tamnd/rucc/tar.gz/$commit" | tar -xzf - -C /build/rucc --strip-components 1
	(cd /build/rucc && rustup toolchain install && CARGO_TARGET_DIR=/build/rucc/target cargo build -q --release -p rucc)
	cp /build/rucc/target/release/rucc "$bin/"
	{
		echo
		echo "# rucc, tamnd/rucc $commit"
		"$bin/rucc" --version 2>&1 | head -3
	} >> "$out/rb-tools.txt"
	rucc_flags="--rucc $bin/rucc --ungraded"
fi

{
	echo
	echo "# rb toolchain $RB_PIN_NAME --cc $side, in $(($(date +%s) - start))s from the start of rb-tools.sh"
} >> "$out/rb-tools.txt"
# shellcheck disable=SC2086
"$bin/rb" toolchain "$RB_PIN_NAME" --dir /build/toolchain --cc "$side" --log "$RB_COMPILE_JSONL" \
	--shim "$bin/rb-cc" $rucc_flags >> "$out/rb-tools.txt" 2>&1 || status=$?
cat "$out/rb-tools.txt"
exit "${status:-0}"
