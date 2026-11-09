#!/bin/sh
# rb census and rb demands over the compile.jsonl of a build. Run as root in the builder VM after
# the build, because rb demands runs each unit again in its own directory.
#
#   census.sh OUTDIR
#
# Environment: RB_SRC, RB_OBJ, RB_JOBS, RB_HARNESS, RB_PIN_NAME and RB_COMPILE_JSONL as for
# rb-tools.sh, and RB_CONF, the name of the SRCCONF. The census leaves out WORLDTMP, where the host
# compiler built the bootstrap tools. OUTDIR gets census.txt, census-objects.tsv, demands.txt and
# demands.toml. compile.jsonl.xz goes beside compile.jsonl.
set -u
out=$1
: "${RB_HARNESS:=/root/harness}"
: "${RB_PIN_NAME:=freebsd-15.1}"
: "${RB_COMPILE_JSONL:=/build/out/compile.jsonl}"
: "${RB_CONF:=graded}"
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
rb=/build/rb/bin/rb
log=$RB_COMPILE_JSONL
mkdir -p "$out"
cd "$RB_HARNESS" || exit 1
status=0

records=$(wc -l < "$log" | tr -d ' ')
bytes=$(stat -f %z "$log")
echo "compile.jsonl: $records records, $bytes bytes" | tee "$out/compile-jsonl.txt"

start=$(date +%s)
"$rb" census "$log" --objdir "$RB_OBJ" --skip "$RB_OBJ$RB_SRC/amd64.amd64/tmp" \
	--objects "$out/census-objects.tsv" > "$out/census.txt" 2>&1 || status=1
echo "rb census: $(($(date +%s) - start))s" | tee -a "$out/compile-jsonl.txt"
head -60 "$out/census.txt"

start=$(date +%s)
"$rb" demands "$log" --pin "$RB_PIN_NAME" --row amd64 --conf "$RB_CONF" --write \
	--jobs "$RB_JOBS" > "$out/demands.txt" 2>&1 || status=1
echo "rb demands: $(($(date +%s) - start))s" | tee -a "$out/compile-jsonl.txt"
head -60 "$out/demands.txt"
[ -f demands.toml ] && cp demands.toml "$out/"

start=$(date +%s)
xz -T0 -k -f "$log"
echo "compile.jsonl.xz: $(stat -f %z "$log.xz") bytes, $(($(date +%s) - start))s" | tee -a "$out/compile-jsonl.txt"
exit "$status"
