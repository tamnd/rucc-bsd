#!/usr/bin/env bash
# The link and the test of one step of rb mixed, for the GENERIC kernel (plan 12.7).
#
#   mixed.sh link
#   mixed.sh test RB OUT CASE REPEAT
#
# netbsd-r.yml builds the GENERIC kernel with the ref TOOLDIR, so that the kernel's object
# directory has its Makefile and generated files, and puts G's sets in the release directory, as
# the image job does. rb mixed then puts R's or G's object of each unit in that directory, and
# runs these two commands.
#
# "link" removes the kernel and links it again with make. When nbctfmerge fails,
# ctfmerge-check.sh finishes the link without CTF. It exits 0 when there is a kernel.
#
# "test" makes kern-GENERIC from that kernel (kernset.sh), puts it in the sets
# (kern-into-sets.sh), makes the image with rb image at RB, and boots it with rb boot. The boot
# runs the ATF directory of CASE REPEAT times. CASE is DIR/PROGRAM:CASE, as atf-repeat.py names
# a case, such as net/carp/t_basic:carp_handover_ipv6_halt_nocarpdevip. The test exits 0 when
# the case passed at least once, 1 when it ran and never passed, and 125 when it did not run,
# such as when the image was not made or the kernel did not boot. G passes some cases only at
# times, so REPEAT is more than one. The output of the step goes to OUT/step-K, where K is
# RB_MIXED_R, the number of units with R's object.

. "$(dirname "$0")/common.sh"

tooldir=$NB_ROOT/tools-r
dir=$NB_OBJ/sys/arch/$NB_MACHINE/compile/GENERIC

case $1 in
link)
	log=$NB_LOGS/mixed-link.log
	rm -f "$dir/netbsd" "$dir/netbsd.gdb"
	if ! "$tooldir/bin/nbmake-$NB_MACHINE" -C "$dir" netbsd > "$log" 2>&1; then
		tail -5 "$log" >&2
		NB_TOOLDIR=$tooldir "$NB_HERE/ctfmerge-check.sh" GENERIC "$log" >> "$NB_LOGS/mixed-ctfmerge.md"
	fi
	[ -f "$dir/netbsd" ]
	;;
test)
	rb=$2
	out=$3
	name=$4
	repeat=$5
	step=$out/step-${RB_MIXED_R:-0}
	prog=${name%%:*}
	tcase=${name#*:}
	tdir=${prog%/*}
	prog=${prog##*/}
	binary=$NB_REL/$NB_MACHINE/binary
	img=$NB_ROOT/mixed.img
	rm -rf "$step" "$img" "$NB_ROOT/mixed-image"
	mkdir -p "$step"
	{
		NB_TOOLDIR=$tooldir "$NB_HERE/kernset.sh" GENERIC &&
			"$NB_HERE/kern-into-sets.sh" "$binary" "$binary/sets/kern-GENERIC.tar.xz" &&
			"$rb" image --build "$NB_TOOLS" --out "$img" --work "$NB_ROOT/mixed-image"
	} > "$step/image.log" 2>&1 || {
		tail -5 "$step/image.log" >&2
		exit 125
	}
	truncate -s 12G "$img"
	"$rb" boot --image "$img" --out "$step/boot" --suite boot,tests --tests-only "$tdir" \
		--tests-repeat "$repeat" --timeout 3600 > "$step/boot.txt" 2>&1 || true
	rm -rf "$img" "$NB_ROOT/mixed-image"
	# How many times the case passed, and how many times it ended.
	read -r passed ended < <(find "$step/boot/results/tests/atf" -name '*.tps' -print0 2>/dev/null |
		xargs -0 -r cat | awk -v p="$prog" -v c="$tcase" '
			/^tp-start: / { split($0, a, ", "); prog = a[2] }
			/^tc-end: / {
				split(substr($0, 9), b, ", ")
				if (prog == p && b[2] == c) { n++; if (b[3] == "passed") ok++ }
			}
			END { print ok + 0, n + 0 }')
	echo "mixed.sh: $name passed $passed of $ended" | tee "$step/verdict" >&2
	if [ "$passed" -gt 0 ]; then exit 0; fi
	if [ "$ended" -eq 0 ]; then exit 125; fi
	exit 1
	;;
*)
	echo "usage: mixed.sh link | test RB OUT CASE REPEAT" >&2
	exit 2
	;;
esac
