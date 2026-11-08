#!/usr/bin/env bash
# The control of an R TOOLDIR, installed by "scripts/netbsd/overlay.sh ref" in place of shim-cc.sh
# and shim-cpp.sh. It runs the reference program that overlay.sh renamed to NAME.ref, with the
# arguments unchanged, and writes the log line of shim-cc.sh. A kernel built through it must be
# the G kernel. If it is not, the overlay and not rucc made the difference.

log=${RB_SHIM_LOG-@LOG@}
"$0.ref" "$@"
status=$?
if [ -n "$log" ]; then
	line="$status	$PWD	@SLOT@"
	for a in "$@"; do line+="	$a"; done
	printf '%s\n' "$line" >> "$log"
fi
exit $status
