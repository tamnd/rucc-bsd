#!/bin/sh
# Build rb-bench for both sides (plan 13.2).
#
#   build.sh OUTDIR "G-CC" "R-CC" [FLAG ...]
#
# G-CC and R-CC are compiler command lines, split at spaces. Each compiles bench/rb-bench.c with -O2
# and the FLAGs to an object. G-CC then links both objects with the same crt files and libraries,
# so only the code of rb-bench differs between OUTDIR/rb-bench-g and OUTDIR/rb-bench-r.
# OUTDIR/build.txt records the commands and the sizes. It runs under sh on FreeBSD and bash on
# Linux.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
src=$here/../../bench/rb-bench.c
out=$1
g=$2
r=$3
shift 3
mkdir -p "$out"
: > "$out/build.txt"
for side in g r; do
	if [ "$side" = g ]; then cc=$g; else cc=$r; fi
	echo "$side: $cc -O2 $* -c rb-bench.c" >> "$out/build.txt"
	# shellcheck disable=SC2086
	$cc -O2 "$@" -c "$src" -o "$out/rb-bench-$side.o"
	# shellcheck disable=SC2086
	$g "$@" "$out/rb-bench-$side.o" -o "$out/rb-bench-$side" -lpthread -lm
done
echo "link: $g $* OBJECT -lpthread -lm" >> "$out/build.txt"
ls -l "$out"/rb-bench-g "$out"/rb-bench-r >> "$out/build.txt"
cat "$out/build.txt"
