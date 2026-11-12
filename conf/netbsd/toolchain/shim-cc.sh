#!/usr/bin/env bash
# The C compiler of an R TOOLDIR, installed as bin/x86_64--netbsd-gcc by scripts/netbsd/overlay.sh.
# It runs rucc for the NetBSD target with the arguments the tree passes, plus what the in-tree GCC
# has built in and the tree does not pass:
#   --target and -fgnuc-version: the triple and the persona of docs/plan 04.3 and 05.4;
#   --sysroot=DESTDIR: tools/gcc/Makefile configures the in-tree GCC with --with-sysroot=DESTDIR,
#   so the reference uses DESTDIR when the command line has no --sysroot. bsd.sys.mk passes one for
#   the world; the kernel passes -nostdinc and does not need one;
#   -B TOOLDIR/x86_64--netbsd/bin: the in-tree GCC runs the ld of its tooldir for each link, and
#   the tree does not name a linker. rucc looks for ld in the -B directories first. Without them it
#   finds no NetBSD ld and runs the host's ld.lld. ld.lld -r puts the __start_link_set_* symbols of
#   the kmodule script at the end of the section before the set, so a module that iterates a link
#   set reads the wrong memory (amdgpu panics in drm_sysctl_init). rucc reads -B only for a link.
# Each call appends one line to the log: the exit status, the directory, the slot, then the
# arguments, all separated by tabs. RB_SHIM_LOG names another log, or none when it is empty.
# With a log, and GNU time on the host (overlay.sh puts its path in timer), each call also appends
# "WALL USER SYSTEM MAXRSS<TAB>LOG LINE" to LOG.rusage: the seconds and the peak RSS in KiB that
# GNU time gives. shim-records.py puts them in the records, for rb speed.
# RB_SHIM_REF is a comma separated list of directories of the tree, such as tests/lib/libc/misc.
# A call whose working directory is in one of them, the source or the object directory, runs the
# reference program that overlay.sh renamed to NAME.ref with the arguments unchanged, and its log
# line has cc-ref in the third field. A build that sets it is not an R build of those directories.
# The rb-cc shim of docs/plan 05 replaces this file when it exists.

rucc=@RUCC@
log=${RB_SHIM_LOG-@LOG@}
timer=@TIME@
use=
if [ -n "$log" ] && [ -n "$timer" ]; then use=$(mktemp "${TMPDIR:-/tmp}/rb-shim.XXXXXXXX"); fi
run() {
	if [ -n "$use" ]; then "$timer" -f '%e %U %S %M' -o "$use" "$@"; else "$@"; fi
}
args=(--target=@TARGET@ -fgnuc-version=@GNUC@ -B@LD_DIR@)
sysroot=--sysroot=@SYSROOT@
for a in "$@"; do
	case $a in --sysroot | --sysroot=*) sysroot= ;; esac
done
slot=cc
IFS=, read -ra ref_dirs <<< "${RB_SHIM_REF-}"
for d in "${ref_dirs[@]}"; do
	case $PWD/ in *"/${d%/}/"*) slot=cc-ref ;; esac
done
if [ "$slot" = cc-ref ]; then
	run "$0.ref" "$@"
else
	run "$rucc" "${args[@]}" ${sysroot:+"$sysroot"} "$@"
fi
status=$?
if [ -n "$log" ]; then
	line="$status	$PWD	$slot"
	for a in "$@"; do line+="	$a"; done
	printf '%s\n' "$line" >> "$log"
fi
if [ -n "$use" ]; then
	printf '%s\t%s\n' "$(tail -n 1 "$use")" "$line" >> "$log.rusage"
	rm -f "$use"
fi
exit $status
