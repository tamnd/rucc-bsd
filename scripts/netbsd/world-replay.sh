#!/usr/bin/env bash
# Compile every unit of a world built through the control again with rucc, as freebsd-r does.
#
#   world-replay.sh RUCC
#
# Run it after "overlay.sh ref" and "build.sh distribution" through that TOOLDIR. The shim log of
# that build ($NB_LOGS/shim.tsv) then has every call of the C compiler slot. The script:
#   1. keeps that log as shim-ref.tsv and writes it as compile-ref.jsonl with shim-records.py;
#   2. runs "overlay.sh RUCC", so that the C compiler slot of the R TOOLDIR runs RUCC and the shim
#      log starts again empty;
#   3. writes the replay scripts with scripts/freebsd/replay.py: each unit (a call with -c and a
#      C or assembly source) runs again in its directory through the rucc shim, with the object
#      under $NB_ROOT/replay-obj and the standard error next to it as .err;
#   4. runs the scripts, one for each CPU.
#
# Every unit thus gets a rucc result, also the units that a failed unit before it would hide in a
# rucc build. The tree, the object directory and DESTDIR are those of the control, so each unit
# finds the generated files and the installed headers it found there. A unit stops after 300 s and
# its source goes to $NB_LOGS/replay-timeouts.txt. replay-report.py reads the results. The nbmake
# wrapper of the control build is kept as $NB_ROOT/nbmake-r for stand.sh.

. "$(dirname "$0")/common.sh"

rucc=$1
gnu=$(sed -n 's/^MACHINE_GNU_PLATFORM=//p' "$NB_TOOLS/bin/nbmake-$NB_MACHINE" | head -1)
gnu=${gnu:-x86_64--netbsd}
out=$NB_ROOT/replay

cp "$NB_LOGS/shim.tsv" "$NB_LOGS/shim-ref.tsv"
# overlay.sh removes the nbmake wrapper of the control build, and stand.sh needs it.
cp "${NB_TOOLDIR_R:-$NB_ROOT/tools-r}/bin/nbmake-$NB_MACHINE" "$NB_ROOT/nbmake-r"
python3 -I "$NB_HERE/shim-records.py" "$NB_LOGS/shim-ref.tsv" G "$NB_OBJ" "$NB_OBJ" \
	"$NB_LOGS/shim.tsv.err" > "$NB_LOGS/compile-ref.jsonl"
"$NB_HERE/overlay.sh" "$rucc"

: > "$NB_LOGS/replay-timeouts.txt"
python3 -I "$NB_REPO/scripts/freebsd/replay.py" "$NB_LOGS/compile-ref.jsonl" \
	--shim "${NB_TOOLDIR_R:-$NB_ROOT/tools-r}/bin/$gnu-gcc" --out "$out" \
	--objects "$NB_ROOT/replay-obj" --shards "$NB_JOBS" \
	--timeouts "$NB_LOGS/replay-timeouts.txt" --stderr | tee "$NB_LOGS/replay.txt"

start=$(date +%s)
for f in "$out"/replay-*.sh; do sh "$f" & done
wait
echo "replay: $(($(date +%s) - start)) s" | tee -a "$NB_LOGS/replay.txt"
