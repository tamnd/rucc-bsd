#!/usr/bin/env bash
# The files that the c-only row borrows from the graded R build of the same pin (plan 10.5).
#
#   borrow.sh RSETS CSETS LIST
#
# MKCXX=no turns off MKATF, MKKYUA, MKGCCCMDS, MKGDB and MKGROFF (share/mk/bsd.own.mk). So the
# c-only build has no tests set, and no ATF or kyua to run the tests. RSETS is the sets directory
# of the graded R build, and CSETS that of the c-only build. The script adds two sets to CSETS:
#   tests     the tests set of RSETS, as it is;
#   borrowed  each path of the base set of RSETS that the base set of CSETS does not have. These
#             are ATF, kyua, lutok and the C++ libraries that they need.
# The borrowed set also has etc/mtree/set.borrowed. It has a line for each borrowed path that no
# mtree file of the etc set of CSETS has, from the mtree files of the etc set of RSETS. The
# directories are in NetBSD.dist and the files in set.base and set.tests. The keywords of /set
# are written out in each line, so the line means what it meant in its own file. The live image
# makes the makefs spec from etc/mtree/*, and makefs leaves out each path that the spec does not
# name, so the borrowed paths need these lines.
#
# LIST gets one line for each borrowed path, the set and the path, for the census. Standard
# output gets one line, the sets for the live image: those of SETS_DEFAULT in
# distrib/common/bootimage/Makefile.bootimage that CSETS has, then tests and borrowed. rb image
# runs make with its environment, so SETS=LINE gives them to the live image.
set -euo pipefail
r=$(cd "$1" && pwd)
c=$(cd "$2" && pwd)
list=$3

# The file of set $2 in directory $1.
set_file() {
	local f
	for f in "$1/$2.tgz" "$1/$2.tar.xz"; do
		if [ -f "$f" ]; then
			echo "$f"
			return 0
		fi
	done
	echo "borrow.sh: $1 has no $2 set" >&2
	return 1
}

# The paths of a set, each as ./PATH with no / at the end.
paths() {
	tar -tf "$1" | sed -e 's,/*$,,' -e 's,^\([^.]\),./\1,' -e '/^\.$/d' | LC_ALL=C sort -u
}

rbase=$(set_file "$r" base)
cbase=$(set_file "$c" base)
case $cbase in
*.tgz) suffix=tgz z=-z ;;
*) suffix=tar.xz z=-J ;;
esac
rtests=$(set_file "$r" tests)
if [ "${rtests##*/}" != "tests.$suffix" ]; then
	echo "borrow.sh: the R tests set is ${rtests##*/}, and the c-only sets end in .$suffix" >&2
	exit 1
fi
for s in tests borrowed; do
	if [ -e "$c/$s.$suffix" ]; then
		echo "borrow.sh: $c already has $s.$suffix" >&2
		exit 1
	fi
done

tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT
paths "$rbase" > "$tmp/r-base"
paths "$cbase" > "$tmp/c-base"
LC_ALL=C comm -23 "$tmp/r-base" "$tmp/c-base" > "$tmp/base"
paths "$rtests" > "$tmp/tests"

mkdir "$tmp/r-etc" "$tmp/c-etc" "$tmp/root"
tar -xf "$(set_file "$r" etc)" -C "$tmp/r-etc"
tar -xf "$(set_file "$c" etc)" -C "$tmp/c-etc"
for f in NetBSD.dist set.base set.tests; do
	if [ ! -f "$tmp/r-etc/etc/mtree/$f" ]; then
		echo "borrow.sh: the etc set of $r has no etc/mtree/$f" >&2
		exit 1
	fi
done
# The paths that the spec of the c-only image has already.
awk '$1 ~ /^\.\// { print $1 }' "$tmp/c-etc"/etc/mtree/* | LC_ALL=C sort -u > "$tmp/c-spec"
LC_ALL=C sort -u "$tmp/base" "$tmp/tests" > "$tmp/want"
LC_ALL=C comm -23 "$tmp/want" "$tmp/c-spec" > "$tmp/new"

if [ -s "$tmp/base" ]; then
	tar -xf "$rbase" -C "$tmp/root" --no-recursion -T "$tmp/base"
fi
mkdir -p "$tmp/root/etc/mtree"
spec=$tmp/root/etc/mtree/set.borrowed
{
	echo "# The paths that the c-only image borrows from the graded R build (scripts/netbsd/borrow.sh)."
	awk '
	NR == FNR { want[$1] = 1; next }
	FNR == 1 { split("", set); split("", known); nk = 0 }
	$1 == "/set" {
		for (i = 2; i <= NF; i++) {
			split($i, kv, "=")
			if (!(kv[1] in known)) { known[kv[1]] = 1; keys[++nk] = kv[1] }
			set[kv[1]] = $i
		}
		next
	}
	$1 == "/unset" {
		for (i = 2; i <= NF; i++) delete set[$i]
		next
	}
	($1 in want) && !seen[$1]++ {
		split("", own)
		for (i = 2; i <= NF; i++) { split($i, kv, "="); own[kv[1]] = 1 }
		line = $1
		for (j = 1; j <= nk; j++)
			if ((keys[j] in set) && !(keys[j] in own)) line = line " " set[keys[j]]
		for (i = 2; i <= NF; i++) line = line " " $i
		print line
	}' "$tmp/new" "$tmp/r-etc"/etc/mtree/*
	echo "./etc/mtree/set.borrowed type=file uname=root gname=wheel mode=0444"
} > "$spec"
chmod 444 "$spec"
(cd "$tmp/root" && tar -c $z -f "$c/borrowed.$suffix" .)
cp "$rtests" "$c/tests.$suffix"

{
	sed 's/^/tests	/' "$tmp/tests"
	sed 's/^/base	/' "$tmp/base"
} > "$list"
echo "borrow.sh: $(awk 'END { print NR }' "$tmp/tests") paths of the tests set and $(awk 'END { print NR }' "$tmp/base") of the base set" >&2

sets=
for s in modules base etc comp games gpufw man misc rescue text base32 base64 manhtml \
	xbase xcomp xetc xfont xserver; do
	if set_file "$c" "$s" > /dev/null 2>&1; then
		sets="$sets $s"
	fi
done
echo "${sets# } tests borrowed"
