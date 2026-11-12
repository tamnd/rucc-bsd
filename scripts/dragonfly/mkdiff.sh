#!/bin/sh
# The make variables of the DragonFly tree with the reference CCVER and with the CCVER of rucc, and
# their differences (plan 05.5 and open question 10). Run in the builder VM after the source is in
# RB_SRC. It is the DragonFly form of rb mk-diff, which reads FreeBSD and NetBSD only.
#
#   mkdiff.sh OUTDIR REFCCVER CCVER
#
# share/mk keys the warning sets and -Werror on the name of CCVER (bsd.sys.mk), the CPU flags on
# it (bsd.cpu.mk), and the kernel code flags on it (sys/platform/pc64/conf/kern.mk, kern.pre.mk
# and kmod.mk). make -V reads each variable at each point, with the make files of the tree, three
# times:
# - REF: CCVER=REFCCVER, the configuration of G.
# - name: CCVER=CCVER only, which shows what the name of the CCVER changes.
# - CCVER: CCVER=CCVER with the variables of ccver.sh, which is how build.sh builds R.
# OUTDIR/mk-*.tsv hold the values. OUTDIR/mk-diff-name.txt holds the lines of the name that differ
# from REF, and OUTDIR/mk-diff.txt the lines of the build of R that differ from REF. The step
# fails if mk-diff.txt has a difference.
set -u
out=$1
ref=$2
ccver=$3
mkdir -p "$out"
RB_CCVER=$ccver RB_CCVER_REF=$ref
# shellcheck source=scripts/dragonfly/ccver.sh
. "$(dirname "$0")/ccver.sh"
ccver_make=$(rb_ccver_make)
# The points: libraries, programs and a kernel module. lib/libzstd and sbin/devd test the name of
# CCVER in their own Makefile.
points="lib/libc lib/libzstd bin/sh sbin/devd sys/vfs/ext2fs"
# WERROR is not a variable here: only sys/conf uses it, and CFLAGS of the module holds it.
vars="CC CFLAGS CWARNFLAGS COPTFLAGS MACHINE_CPU _CPUCFLAGS WARNS NO_WERROR"

# mk TSV CCVER [VAR=VALUE ...]
mk() {
	tsv=$1 v=$2
	shift 2
	: > "$tsv"
	for p in $points; do
		for n in $vars; do
			value=$(cd "$RB_SRC/$p" && env CCVER="$v" HOST_CCVER="$ref" make -m "$RB_SRC/share/mk" \
				__MAKE_CONF=/dev/null "$@" -V "$n" 2>&1 | tr '\t' ' ' | tr -s ' ')
			printf '%s\t%s\t%s\n' "$p" "$n" "$value" >> "$tsv"
		done
	done
}
mk "$out/mk-$ref.tsv" "$ref"
mk "$out/mk-$ccver-name.tsv" "$ccver"
# shellcheck disable=SC2086
mk "$out/mk-$ccver.tsv" "$ccver" $ccver_make
{
	echo "# make -V at $points, CCVER=$ref (<) and CCVER=$ccver (>)"
	diff "$out/mk-$ref.tsv" "$out/mk-$ccver-name.tsv" || true
} > "$out/mk-diff-name.txt"
{
	echo "# make -V at $points, CCVER=$ref (<) and CCVER=$ccver $ccver_make (>)"
	diff "$out/mk-$ref.tsv" "$out/mk-$ccver.tsv"
} > "$out/mk-diff.txt"
status=$?
cat "$out/mk-diff-name.txt" "$out/mk-diff.txt"
exit "$status"
