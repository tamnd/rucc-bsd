#!/bin/sh
# One phase of a FreeBSD build, run as root in the builder VM.
#
#   build.sh world | kernel | toolchain
#
# Environment:
#   RB_SRC       the tree, unmodified
#   RB_OBJ       MAKEOBJDIRPREFIX, the only place the build writes
#   RB_SRCCONF   the SRCCONF file, conf/freebsd/<conf>.conf
#   RB_TOOLCHAIN the CROSS_TOOLCHAIN name, amd64-gcc15, or the path of a .mk file of rb toolchain
#   RB_OUT       where logs and phases.tsv go
#   RB_JOBS      make -j
#   RB_KERNCONF  the kernel configuration, GENERIC by default
#   RB_MAKEFLAGS more make flags, such as -k
#   RB_PHASE     the name of the phase in the logs, the argument by default
#
# The make command line is the one FreeBSD's own GCC CI jobs use (freebsd-ci, jobs/*-gcc14_build),
# with our SRCCONF. Each phase appends "phase status seconds obj_kb disk_used_kb" to phases.tsv. On a
# failure, PHASE-error.txt holds the lines around the first error in the log.
set -u
phase=$1
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
: "${RB_KERNCONF:=GENERIC}"
mkdir -p "$RB_OUT" "$RB_OBJ"
export MAKEOBJDIRPREFIX="$RB_OBJ"

case $phase in
world) target=buildworld ;;
kernel) target=buildkernel ;;
toolchain) target=kernel-toolchain ;;
*) echo "usage: $0 world | kernel | toolchain" >&2; exit 2 ;;
esac
phase=${RB_PHASE:-$phase}

log="$RB_OUT/$phase.log"
start=$(date +%s)
echo "$phase: make -j$RB_JOBS ${RB_MAKEFLAGS:+$RB_MAKEFLAGS }$target CROSS_TOOLCHAIN=$RB_TOOLCHAIN SRCCONF=$RB_SRCCONF" \
	"in $RB_SRC, started $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
# Only the stage banners reach the job log. The whole log is kept as an artifact.
{
	# shellcheck disable=SC2086
	make -C "$RB_SRC" -j"$RB_JOBS" ${RB_MAKEFLAGS:-} "$target" \
		TARGET=amd64 TARGET_ARCH=amd64 KERNCONF="$RB_KERNCONF" \
		CROSS_TOOLCHAIN="$RB_TOOLCHAIN" \
		__MAKE_CONF=/dev/null SRCCONF="$RB_SRCCONF" 2>&1
	echo $? > "$RB_OUT/$phase.status"
} | tee "$log" | grep --line-buffered -E '^(>>>|-{10,}|make.*: stopped|ERROR)'
status=$(cat "$RB_OUT/$phase.status")
seconds=$(($(date +%s) - start))

if [ "$status" -ne 0 ]; then
	# The first compiler or make error, with the command before it. With -j the lines of other
	# targets are mixed in, so this is a pointer into the log, not a diagnosis.
	line=$(grep -n -m1 -E '(^|[^-=])error:|Error code|^make\[[0-9]+\]: .*Stop|: stopped' "$log" | cut -d: -f1)
	if [ -n "$line" ]; then
		from=$((line > 40 ? line - 40 : 1))
		sed -n "${from},$((line + 15))p" "$log" > "$RB_OUT/$phase-error.txt"
	else
		tail -60 "$log" > "$RB_OUT/$phase-error.txt"
	fi
	echo "$phase failed with status $status after ${seconds}s. First error:"
	cat "$RB_OUT/$phase-error.txt"
fi

obj_kb=$(du -sk "$RB_OBJ" | cut -f1)
used_kb=$(df -k "$RB_OBJ" | awk 'NR == 2 { print $3 }')
printf '%s\t%s\t%s\t%s\t%s\n' "$phase" "$status" "$seconds" "$obj_kb" "$used_kb" >> "$RB_OUT/phases.tsv"
echo "$phase: status $status, ${seconds}s, objects ${obj_kb} KB, disk used ${used_kb} KB"
exit "$status"
