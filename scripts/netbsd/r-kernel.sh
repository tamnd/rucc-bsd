#!/usr/bin/env bash
# Build one kernel twice through the shims of overlay.sh: once with the reference GCC (the ref
# leg, which is G) and once with rucc (the rucc leg, which is R). Each leg keeps its objects for rb.
#
#   r-kernel.sh KERNEL
#
# NB_PIN_FILE, NB_MACHINE, NB_MACHINE_ARCH and NB_VARS name the pin and the row, as in common.sh.
# TOOLDIR must be built, and rucc must be on PATH.
#
# Each leg leaves these in $NB_ROOT/cmp/LEG:
#   obj/sys/arch/MACHINE/compile/KERNEL   the kernel's object directory, moved out of $NB_OBJ;
#   compile.jsonl                         the records of shim-records.py, with $NB_OBJ moved to
#                                         $NB_ROOT/cmp/LEG/obj, so that rb pairs the two legs;
#   report.md                             the counts and the first failure of r-report.sh;
#   shim.tsv                              the shim log.
#
# When the rucc leg fails, "make -k" goes on with the units that do not depend on the failure,
# so that every unit it can reach gets a result. The script fails only when the ref leg fails,
# because then the overlay or the tree is broken and the rucc leg means nothing.

. "$(dirname "$0")/common.sh"

kernel=$1
dir=sys/arch/$NB_MACHINE/compile/$kernel

for cc in ref rucc; do
	"$NB_HERE/overlay.sh" "$cc"
	out=$NB_ROOT/cmp/$cc
	rm -rf "$out"
	mkdir -p "$out/obj/sys/arch/$NB_MACHINE/compile"
	logs=("$NB_LOGS/kernel-$cc.log")
	status=0
	NB_TOOLDIR=$NB_ROOT/tools-r NB_NOISE=2 "$NB_HERE/phase.sh" "kernel-$cc" -u "kernel=$kernel" \
		|| status=$?
	if [ "$status" != 0 ] && [ "$cc" = ref ]; then
		echo "r-kernel: the ref leg failed, so the overlay or the tree is broken" >&2
		exit 1
	fi
	if [ "$status" != 0 ] && [ -f "$NB_OBJ/$dir/Makefile" ]; then
		NB_TOOLDIR=$NB_ROOT/tools-r "$NB_HERE/phase.sh" "kernel-$cc-k" -- \
			"$NB_ROOT/tools-r/bin/nbmake-$NB_MACHINE" -k -j "$NB_JOBS" -C "$NB_OBJ/$dir" || true
		logs+=("$NB_LOGS/kernel-$cc-k.log")
	fi
	"$NB_HERE/r-report.sh" "${logs[@]}" > "$out/report.md"
	side=G
	if [ "$cc" = rucc ]; then side=R; fi
	python3 "$NB_HERE/shim-records.py" "$NB_LOGS/shim.tsv" "$side" "$NB_OBJ" "$out/obj" \
		> "$out/compile.jsonl"
	cp "$NB_LOGS/shim.tsv" "$out/shim.tsv"
	if [ -d "$NB_OBJ/$dir" ]; then mv "$NB_OBJ/$dir" "$out/obj/$dir"; fi
	echo "r-kernel: the $cc leg of $kernel exited $status"
done
