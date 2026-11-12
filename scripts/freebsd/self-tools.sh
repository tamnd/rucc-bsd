#!/bin/sh
# The shims for a build inside a booted RR image (plan 16.11, B10), run as root after provision.sh
# and after pkg add of the rucc package. The image has no Rust, so rb and rb-cc come built from the
# builder VM, in the tarball that the package job of freebsd-self makes.
#
#   self-tools.sh OUTDIR RB_BIN_TARBALL
#
# rb toolchain then writes /build/toolchain, as rb-tools.sh does in the builder VM, with the rucc of
# the package, /usr/local/bin/rucc, as the compiler of R. First it runs with no --ungraded, so that
# rb checks that the binary of the package is the pinned rucc by its rucc.source. When that check
# fails, it runs again with --ungraded, and OUTDIR/rb-tools.txt says why.
set -eu
out=$1
tarball=$2
: "${RB_HARNESS:=/root/harness}"
: "${RB_PIN_NAME:=freebsd-15.1}"
: "${RB_ROW:=amd64}"
: "${RB_COMPILE_JSONL:=/build/out/compile.jsonl}"
bin=/build/rb/bin
rucc=/usr/local/bin/rucc
mkdir -p "$out" "$bin" "$(dirname "$RB_COMPILE_JSONL")"
tar -xJf "$tarball" -C "$bin"
if [ ! -x "$rucc" ] || [ ! -f "$rucc.source" ]; then
	echo "self-tools.sh: no $rucc with rucc.source; pkg add the rucc package first" >&2
	exit 2
fi
{
	echo "# rb"
	"$bin/rb" version
	echo
	echo "# rucc of the package $(pkg query '%n-%v' rucc), $rucc"
	"$rucc" --version 2>&1 | head -3
	cat "$rucc.source"
	echo
	echo "# rb toolchain $RB_PIN_NAME --row $RB_ROW --cc rucc --rucc $rucc"
} > "$out/rb-tools.txt"
cd "$RB_HARNESS"
run() {
	# shellcheck disable=SC2068
	"$bin/rb" toolchain "$RB_PIN_NAME" --row "$RB_ROW" --dir /build/toolchain --cc rucc --log "$RB_COMPILE_JSONL" \
		--shim "$bin/rb-cc" --rucc "$rucc" $@
}
if run >> "$out/rb-tools.txt" 2>&1; then
	echo "graded: rb takes the rucc of the package as the pinned rucc" >> "$out/rb-tools.txt"
else
	{
		echo
		echo "# the check failed, so again with --ungraded"
	} >> "$out/rb-tools.txt"
	run --ungraded >> "$out/rb-tools.txt" 2>&1 || status=$?
	echo "ungraded: the rucc of the package is not the pinned rucc" >> "$out/rb-tools.txt"
fi
cat "$out/rb-tools.txt"
exit "${status:-0}"
