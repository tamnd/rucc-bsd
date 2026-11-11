#!/usr/bin/env bash
# rb syntax over every C unit of a build through the control (plan 05.7).
#
#   syntax.sh RB NAME
#
# Run it after "overlay.sh ref" and a build.sh through that TOOLDIR, with rucc on PATH. The shim
# log of that build and the standard error that shim-ref.sh kept become a compile.jsonl of side G,
# and rb syntax runs each unit through rucc with -fsyntax-only, in its own directory and with its
# own flags. DESTDIR is the sysroot of each unit that names none, as the shims of rucc give it.
# The report is $NB_LOGS/syntax-NAME.txt and each unit is a line of $NB_LOGS/syntax-NAME.tsv. The
# script fails only when rb cannot run.

. "$(dirname "$0")/common.sh"

rb=$1
name=$2
log=$NB_LOGS/syntax-$name.jsonl
python3 -I "$NB_HERE/shim-records.py" "$NB_LOGS/shim.tsv" G "$NB_OBJ" "$NB_OBJ" \
	"$NB_LOGS/shim.tsv.err" > "$log"
cd "$NB_REPO" || exit
status=0
"$rb" syntax netbsd-trunk --log "$log" --rucc "$(command -v rucc)" --sysroot "$NB_DEST" \
	--jobs "$NB_JOBS" --limit 100 --out "$NB_LOGS/syntax-$name.tsv" \
	> "$NB_LOGS/syntax-$name.txt" 2>&1 || status=$?
if [ "$status" -gt 1 ]; then
	cat "$NB_LOGS/syntax-$name.txt" >&2
	exit "$status"
fi
echo "rb syntax exited $status" >> "$NB_LOGS/syntax-$name.txt"
head -1 "$NB_LOGS/syntax-$name.txt"
