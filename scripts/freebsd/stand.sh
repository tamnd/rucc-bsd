#!/bin/sh
# Build the boot code again with rucc and give its sizes beside those of the ref build, as markdown.
#
#   stand.sh ref
#   stand.sh
#
# Environment: as build.sh, RB_SRC, RB_OBJ, RB_SRCCONF, RB_OUT and RB_JOBS, and RB_TOOLCHAIN, the
# .mk file of the rucc side.
#
# "stand.sh ref" runs after make buildworld with ref.mk. It writes the size of each boot file of
# the list below, taken from the object directory of stand, to stand-ref.tsv.
#
# "stand.sh" runs after it. Through make buildenv with RB_TOOLCHAIN, it runs "make cleandir" and
# "make -k all" in stand, so that the i386 boot blocks (boot2, gptboot, gptzfsboot, isoboot, pmbr
# and the others), the BIOS loaders, the EFI loaders and userboot are compiled again by rucc into
# the same object directory. The objects of the ref build in stand are then gone. Then it takes the
# sizes of the files of stand-ref.tsv again, to stand-rucc.tsv. A file that the rucc build did not
# make has no size. The compiler calls of this build go to RB_COMPILE_LOG,
# /build/out/compile-stand.jsonl by default, and the build log is stand.log.
#
# The limits are those of the Makefiles of stand: boot2.ld at most BOOT2SIZE (7680) bytes, isoboot
# at most ISOBOOTSIZE (30720), and each loader_*.bin at most LOADERSIZE (500000). The MBRs and
# boot1 are one sector each.
set -u
export LC_ALL=C
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
tab=$(printf '\t')
dir=$RB_OBJ$RB_SRC/amd64.amd64/stand

# The boot files, each with its limit in bytes or "-".
files() {
	cat <<-EOF
	mbr	512
	pmbr	512
	boot0	512
	boot0sio	512
	boot1	512
	boot2.ld	7680
	boot	-
	gptboot	-
	gptzfsboot	-
	isoboot	30720
	cdboot	-
	pxeboot	-
	loader_lua.bin	500000
	loader_4th.bin	500000
	loader_simp.bin	500000
	loader_lua	-
	loader_lua.efi	-
	loader_4th.efi	-
	loader_simp.efi	-
	loader_ia32.efi	-
	boot1.efi	-
	gptboot.efi	-
	userboot_lua.so	-
	userboot_4th.so	-
	EOF
}

# For each line "name limit" of the standard input, "name limit size path", with the size and the
# path of the first file NAME under the object directory. Both are empty when there is none.
sizes() {
	while read -r name limit; do
		path=$(find "$dir" -type f -name "$name" 2>/dev/null | sort | head -1)
		size=
		[ -n "$path" ] && size=$(stat -f %z "$path")
		printf '%s\t%s\t%s\t%s\n' "$name" "$limit" "$size" "${path#"$dir"/}"
	done
}

case ${1:-} in
ref)
	files | sizes > "$RB_OUT/stand-ref.tsv"
	exit
	;;
inside)
	# In make buildenv, which has the environment of the world build with RB_TOOLCHAIN.
	make cleandir > "$RB_OUT/stand-clean.log" 2>&1
	make -k -j"$RB_JOBS" all > "$RB_OUT/stand.log" 2>&1
	echo $? > "$RB_OUT/stand.status"
	exit
	;;
esac

export MAKEOBJDIRPREFIX="$RB_OBJ"
: "${RB_COMPILE_LOG:=/build/out/compile-stand.jsonl}"
export RB_COMPILE_LOG
: > "$RB_COMPILE_LOG"
echo 1 > "$RB_OUT/stand.status"
make -C "$RB_SRC" buildenv TARGET=amd64 TARGET_ARCH=amd64 CROSS_TOOLCHAIN="$RB_TOOLCHAIN" \
	__MAKE_CONF=/dev/null SRCCONF="$RB_SRCCONF" BUILDENV_DIR="$RB_SRC/stand" \
	BUILDENV_SHELL="sh $0 inside" > "$RB_OUT/stand-env.log" 2>&1
status=$(cat "$RB_OUT/stand.status")
cut -f1,2 "$RB_OUT/stand-ref.tsv" | sizes > "$RB_OUT/stand-rucc.tsv"
calls=$(wc -l < "$RB_COMPILE_LOG" | tr -d ' ')
failed=$(grep -cE '"exit": *(-|[1-9])' "$RB_COMPILE_LOG" || true)

echo "### The boot code with rucc"
echo
echo "make -k all in stand exited $status. $failed of $calls compiler calls failed."
echo
echo "| File | Limit | ref | rucc | rucc minus ref | Free with rucc | Object |"
echo "|---|---|---|---|---|---|---|"
# The columns are name, limit, size and path of the ref build, then the same of the rucc build.
paste "$RB_OUT/stand-ref.tsv" "$RB_OUT/stand-rucc.tsv" | awk -F "$tab" '$3 != "" {
	r = ($7 == "") ? "not built" : $7
	d = ($7 == "") ? "" : $7 - $3
	free = ($2 == "-" || $7 == "") ? "" : $2 - $7
	if (free != "" && free < 0) free = free " (too large)"
	printf "| %s | %s | %s | %s | %s | %s | `%s` |\n", $1, $2, $3, r, d, free, $4
}'
echo
echo "The lines of the build log on the size limits and the first errors:"
echo
echo '```'
grep -E 'bytes available|error:|\*\*\* ' "$RB_OUT/stand.log" | head -60 || true
echo '```'
