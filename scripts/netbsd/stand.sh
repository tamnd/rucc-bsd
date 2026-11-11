#!/usr/bin/env bash
# Build the boot code again with rucc and give its sizes beside those of the control, as markdown.
#
#   stand.sh
#
# Run it after world-replay.sh. The R TOOLDIR then runs rucc, and world-replay.sh kept the nbmake
# wrapper of the control build as $NB_ROOT/nbmake-r. The boot code is sys/arch/i386/stand, which
# the amd64 world builds and installs in /usr/mdec: the MBRs, bootxx_* (at most 15 sectors),
# /usr/mdec/boot, the CD, FAT, DOS and PXE loaders and the EFI loaders.
#
# The script takes the size of each file of DESTDIR/usr/mdec from the object directory of the
# control. Then it runs "make cleandir" and "make -k dependall" there through the R TOOLDIR, and
# takes the sizes again. A file that the rucc build did not make has no size. The calls of this
# build go to shim-stand.tsv, so that the shim log of the replay stays as it is. The build log is
# stand.log, and its lines on the size limits are in the output.

. "$(dirname "$0")/common.sh"

export LC_ALL=C
tab=$(printf '\t')
src=$NB_SRC/sys/arch/i386/stand
dir=$NB_OBJ/sys/arch/i386/stand
make=$NB_ROOT/nbmake-r

# The size and the path of the first file NAME under the object directory, outside the libraries.
size_of() {
	find "$dir" -path "$dir/*/lib" -prune -o -type f -name "$1" -printf '%s\t%P\n' 2>/dev/null \
		| sort -t "$tab" -k2 | head -1
}

sizes() {
	for file in "$NB_DEST"/usr/mdec/*; do
		printf '%s\t%s\n' "${file##*/}" "$(size_of "${file##*/}")"
	done
}

sizes > "$NB_LOGS/stand-ref.tsv"
export RB_SHIM_LOG=$NB_LOGS/shim-stand.tsv
: > "$RB_SHIM_LOG"
"$make" -C "$src" cleandir > "$NB_LOGS/stand-clean.log" 2>&1 || true
set +e
"$make" -k -j "$NB_JOBS" -C "$src" dependall > "$NB_LOGS/stand.log" 2>&1
status=$?
set -e
sizes > "$NB_LOGS/stand-rucc.tsv"

echo "### The boot code with rucc"
echo
echo "make -k dependall in sys/arch/i386/stand exited $status. $(awk -F '\t' '$1 != 0' "$RB_SHIM_LOG" | wc -l) of $(wc -l < "$RB_SHIM_LOG") compiler calls failed."
echo
echo "| File of /usr/mdec | Control | rucc | rucc minus control | Object |"
echo "|---|---|---|---|---|"
join -t "$tab" -o 0,1.2,1.3,2.2,2.3 -e '' "$NB_LOGS/stand-ref.tsv" "$NB_LOGS/stand-rucc.tsv" \
	| awk -F '\t' '{
		d = ($2 != "" && $4 != "") ? $4 - $2 : ""
		path = ($3 != "") ? $3 : $5
		printf "| %s | %s | %s | %s | `%s` |\n", $1, $2, ($4 == "" ? "not built" : $4), d, path
	}'
echo
echo "The lines of the build log on the size limits:"
echo
echo '```'
grep -E 'size [0-9]+, -?[0-9]+ free|larger than|error:|\*\*\* ' "$NB_LOGS/stand.log" | head -60 || true
echo '```'
