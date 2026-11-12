#!/usr/bin/env bash
# Make the R TOOLDIR of docs/plan 05.4 from the G TOOLDIR that "build.sh tools" made.
#
#   overlay.sh RUCC
#   overlay.sh ref
#
# The R TOOLDIR ($NB_ROOT/tools-r) is a copy of $NB_TOOLS in which two names change. The names
# carry the GNU platform of the tools (nb_gnu), x86_64--netbsd for amd64:
#   bin/x86_64--netbsd-gcc   is conf/netbsd/toolchain/shim-cc.sh, which runs RUCC;
#   bin/x86_64--netbsd-cpp   is conf/netbsd/toolchain/shim-cpp.sh, which runs RUCC -E.
# The reference programs stay beside them as x86_64--netbsd-gcc.ref and x86_64--netbsd-cpp.ref.
# x86_64--netbsd-c++ and x86_64--netbsd-g++ are not touched: the C++ slot is the reference GCC
# in R and in G (document 10).
#
# "overlay.sh ref" makes the control: both names are conf/netbsd/toolchain/shim-ref.sh, which runs
# the renamed reference program with the arguments unchanged. A build through the control must
# succeed as G does, or the overlay itself is broken.
#
# The nbmake wrapper is removed, because it names TOOLDIR; build.sh writes it again for the R
# TOOLDIR on its next run. The script then lists every file whose hash differs between the two
# TOOLDIRs in $NB_LOGS/toolchain-diff.txt and fails if the list is not exactly the expected one.

. "$(dirname "$0")/common.sh"

mode=$1
rucc=
if [ "$mode" != ref ]; then rucc=$(command -v "$mode"); fi
dst=${NB_TOOLDIR_R:-$NB_ROOT/tools-r}
gnu=$(nb_gnu "$NB_TOOLS")
target=${RB_RUCC_TARGET:-$gnu}
# The directory of the tooldir's ld, which the in-tree GCC runs for a link and shim-cc.sh names
# with -B.
ld_dir=$dst/$gnu/bin
if [ ! -x "$NB_TOOLS/$gnu/bin/ld" ]; then
	echo "overlay: $NB_TOOLS/$gnu/bin/ld is not there, and shim-cc.sh links with it" >&2
	exit 1
fi

export LC_ALL=C
# tools/Makefile.gnuhost configures a GNU tool again when the "uname -srm" in its .configure_done
# is not that of the host. The configure then takes the TOOLDIR of the build as its prefix, and the
# install puts the reference GCC over the shim in the R TOOLDIR. A cache of the tools from a runner
# with another kernel did this in the middle of an R build, and the rest of that build used GCC.
# So the tools are made current for this host in the G TOOLDIR before the copy.
platform=$(uname -srm | tr ' ()' '-')
stale=$(find "$NB_OBJ/tools" -name .configure_done -exec grep -LxF -- "$platform" {} + 2> /dev/null | head -1)
if [ -n "$stale" ]; then
	echo "overlay: $stale does not name $platform, so build.sh tools runs first in $NB_TOOLS"
	NB_TOOLDIR='' nb_buildsh -u tools
fi
: > "$NB_LOGS/shim.tsv"
: > "$NB_LOGS/shim.tsv.rusage"
rm -rf "$NB_LOGS/shim.tsv.err"
# The shims time each call with GNU time when the host has it, for the rusage of the records.
timer=
if /usr/bin/time -f %e -o /dev/null true 2> /dev/null; then timer=/usr/bin/time; fi
rm -rf "$dst"
cp -a "$NB_TOOLS" "$dst"
rm -f "$dst/bin/nbmake-$NB_MACHINE" "$dst/bin/nbmake-$NB_MACHINE-"*

for slot in cc:gcc cpp:cpp; do
	shim=${slot%%:*}
	name=$gnu-${slot#*:}
	mv "$dst/bin/$name" "$dst/bin/$name.ref"
	if [ "$mode" = ref ]; then shim=ref; fi
	sed -e "s|@SLOT@|${slot%%:*}|" -e "s|@RUCC@|$rucc|" -e "s|@LOG@|$NB_LOGS/shim.tsv|" \
		-e "s|@TARGET@|$target|" -e "s|@GNUC@|$NB_PIN_GCC|" -e "s|@SYSROOT@|$NB_DEST|" -e "s|@TIME@|$timer|" \
		-e "s|@LD_DIR@|$ld_dir|" \
		"$NB_REPO/conf/netbsd/toolchain/shim-$shim.sh" > "$dst/bin/$name"
	chmod 755 "$dst/bin/$name"
done

(cd "$NB_TOOLS" && find . -type f ! -name "nbmake-$NB_MACHINE*" -exec sha256sum {} + | sort -k2) \
	> "$NB_LOGS/tooldir-g.sha256"
(cd "$dst" && find . -type f ! -name "nbmake-$NB_MACHINE*" -exec sha256sum {} + | sort -k2) \
	> "$NB_LOGS/tooldir-r.sha256"
join -1 2 -2 2 -a 1 -a 2 -e missing -o 0,1.1,2.1 \
	"$NB_LOGS/tooldir-g.sha256" "$NB_LOGS/tooldir-r.sha256" \
	| awk '$2 != $3 { print $1 }' > "$NB_LOGS/toolchain-diff.txt"
expected=$(printf './bin/%s\n' "$gnu-cpp" "$gnu-cpp.ref" "$gnu-gcc" "$gnu-gcc.ref" | sort)
if [ "$(sort "$NB_LOGS/toolchain-diff.txt")" != "$expected" ]; then
	echo "overlay: the TOOLDIRs differ in more than the two compiler slots:" >&2
	cat "$NB_LOGS/toolchain-diff.txt" >&2
	exit 1
fi

if [ "$mode" = ref ]; then
	echo "overlay: $dst runs the reference $gnu-gcc and $gnu-cpp through shim-ref.sh"
else
	echo "overlay: $dst runs $rucc ($("$rucc" --version 2>&1 | head -1)) as $gnu-gcc and $gnu-cpp"
	echo "overlay: --target=$target -fgnuc-version=$NB_PIN_GCC --sysroot=$NB_DEST -B$ld_dir"
fi
