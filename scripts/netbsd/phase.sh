#!/usr/bin/env bash
# Run one build.sh phase and record its time and the disk it leaves behind.
#
#   phase.sh NAME [build.sh operations and options...]
#   phase.sh NAME -- COMMAND [ARGS...]
#
# The second form times any command instead of build.sh.
#
# The log goes to $NB_LOGS/NAME.log and to standard output. One line is appended to
# $NB_LOGS/phases.tsv with the columns of scripts/netbsd/summary.sh: the phase, the exit status,
# the seconds it took, and the MiB in the source tree, the object directory, TOOLDIR, DESTDIR and
# RELEASEDIR after it, and the MiB still free on the file system that holds them.
#
# The exit status is that of build.sh or of COMMAND.

. "$(dirname "$0")/common.sh"

name=$1
shift
log="$NB_LOGS/$name.log"

start=$(date +%s)
set +e
if [ "${1:-}" = -- ]; then
	shift
	"$@" 2>&1 | tee "$log"
else
	nb_buildsh "$@" 2>&1 | tee "$log"
fi
status=${PIPESTATUS[0]}
set -e
end=$(date +%s)

# In an R TOOLDIR of overlay.sh, the C compiler slot must still be the shim after the build. If a
# tool of the build installed GCC there again, the build used GCC and not the compiler of the shim.
if [ -n "${NB_TOOLDIR:-}" ] && gnu=$(nb_gnu "$NB_TOOLDIR" 2> /dev/null) \
	&& [ -e "$NB_TOOLDIR/bin/$gnu-gcc.ref" ] && [ "$(head -c 2 "$NB_TOOLDIR/bin/$gnu-gcc")" != '#!' ]; then
	echo "phase $name: $NB_TOOLDIR/bin/$gnu-gcc is no longer the shim of overlay.sh" | tee -a "$log" >&2
	if [ "$status" -eq 0 ]; then status=1; fi
fi

printf '%s\t%d\t%d\t%d\t%d\t%d\t%d\t%d\t%d\n' "$name" "$status" $((end - start)) \
	"$(nb_du "$NB_SRC")" "$(nb_du "$NB_OBJ")" "$(nb_du "${NB_TOOLDIR:-$NB_TOOLS}")" \
	"$(nb_du "$NB_DEST")" "$(nb_du "$NB_REL")" \
	"$(df -Pm "$NB_ROOT" | awk 'NR == 2 { print $4 }')" >> "$NB_LOGS/phases.tsv"

echo "phase $name: exit $status after $((end - start)) s"
exit "$status"
