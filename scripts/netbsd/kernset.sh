#!/usr/bin/env bash
# Make the kern-GENERIC set from the kernel that "build.sh kernel=GENERIC" built.
#
#   kernset.sh [CONF]
#
# CONF is a configuration name of the tree, GENERIC by default, or the path of a configuration
# file outside the tree, as build.sh takes it. For conf/netbsd/kernel/RB-TEST the set is
# kern-RB-TEST.
#
# The live image extracts kern-GENERIC with the other sets. build.sh makes that set only in
# "release", which also builds six more kernels and the install media. This script runs the
# commands of the kernset-GENERIC target of etc/Makefile in the tree, with the values that the tree's
# make gives for the same variables. We do not call that target, because it depends on snap_pre,
# which depends on distribution and would rebuild the world. As in etc/Makefile, pax gets
# --timestamp MKREPRO_TIMESTAMP when that is set, for a build with -P.

. "$(dirname "$0")/common.sh"

tooldir=${NB_TOOLDIR:-$NB_TOOLS}
make="$tooldir/bin/nbmake-$NB_MACHINE"
conf=${1:-GENERIC}
case $conf in
*/*)
	conffile=$conf
	conf=${conf##*/}
	;;
*)
	conffile=$NB_SRC/sys/arch/$NB_MACHINE/conf/$conf
	;;
esac

# -v expands the value; -V prints it as written.
v() { "$make" -C "$NB_SRC/etc" -v "$1" 2>/dev/null; }
kernobjdir=$(v KERNOBJDIR)
suffix=$(v TAR_SUFF)
compress=$(v COMPRESS_PROGRAM)
pax=$(v TOOL_PAX)
owner=$(v BINOWN)
group=$(v BINGRP)
gzip_flags=$(v GZIP_FLAGS)
xz_opt=$(v XZ_OPT)

timestamp=()
[ -z "${MKREPRO_TIMESTAMP:-}" ] || timestamp=(--timestamp "$MKREPRO_TIMESTAMP")

kerndir="$kernobjdir/$conf"
setdir="$NB_REL/$NB_MACHINE/binary/sets"
set_file="$setdir/kern-$conf.$suffix"
mkdir -p "$setdir"

# The kernel names come from the "config" lines of the kernel configuration, as GETKERNELAWK does.
kernels=$(awk '/^config/ { print $2; found = 1 } END { if (!found) print "netbsd" }' "$conffile")

{
	echo "/set uname=$owner gname=$group"
	echo ". type=dir optional"
	for k in $kernels; do
		[ -f "$kerndir/$k" ] && echo "./$k type=file"
	done
} | (cd "$kerndir" && GZIP="$gzip_flags" XZ_OPT="$xz_opt" \
	"$pax" "${timestamp[@]}" --use-compress-program "$compress" -O -w -M -N "$NB_SRC/etc" -f "$set_file")

echo "kernset: $set_file, $(du -k "$set_file" | cut -f1) KiB"
