#!/bin/sh
# The make variables of the DragonFly tree with the reference CCVER and with the CCVER of rucc, and
# their differences (plan 05.5 and open question 10). Run in the builder VM after the source is in
# RB_SRC. It is the DragonFly form of rb mk-diff, which reads FreeBSD and NetBSD only.
#
#   mkdiff.sh OUTDIR REFCCVER CCVER
#
# share/mk keys the warning sets and -Werror on the name of CCVER (bsd.sys.mk), the CPU flags on
# it (bsd.cpu.mk), and the kernel code flags on it (sys/platform/pc64/conf/kern.mk). make -V
# reads each variable at each point with CCVER set to each name, and with the make files of the
# tree. OUTDIR/mk-REFCCVER.tsv and OUTDIR/mk-CCVER.tsv hold the values, and OUTDIR/mk-diff.txt holds
# the lines that differ.
set -u
out=$1
ref=$2
ccver=$3
mkdir -p "$out"
# The points: a library, a program, a kernel module and the kernel make file of the platform.
points="lib/libc bin/sh sys/vfs/ext2fs"
vars="CC CFLAGS CWARNFLAGS COPTFLAGS MACHINE_CPU _CPUCFLAGS WARNS NO_WERROR"

for v in "$ref" "$ccver"; do
	: > "$out/mk-$v.tsv"
	for p in $points; do
		for n in $vars; do
			value=$(cd "$RB_SRC/$p" && env CCVER="$v" HOST_CCVER="$ref" make -m "$RB_SRC/share/mk" \
				__MAKE_CONF=/dev/null -V "$n" 2>&1 | tr '\t' ' ' | tr -s ' ')
			printf '%s\t%s\t%s\n' "$p" "$n" "$value" >> "$out/mk-$v.tsv"
		done
	done
done
{
	echo "# make -V at $points, CCVER=$ref (<) and CCVER=$ccver (>)"
	diff "$out/mk-$ref.tsv" "$out/mk-$ccver.tsv" || true
} > "$out/mk-diff.txt"
cat "$out/mk-diff.txt"
