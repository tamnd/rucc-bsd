#!/usr/bin/env bash
# The control of an R TOOLDIR, installed by "scripts/netbsd/overlay.sh ref" in place of shim-cc.sh
# and shim-cpp.sh. It runs the reference program that overlay.sh renamed to NAME.ref, with the
# arguments unchanged, and writes the log line of shim-cc.sh. A kernel built through it must be
# the G kernel. If it is not, the overlay and not rucc made the difference.

#
# The reference's standard error also goes to a file in LOG.err, named by the first 16 hex digits
# of the SHA-256 of the log line, so that shim-records.py can give rb the diagnostics of G. A call
# that wrote nothing on standard error leaves no file.

log=${RB_SHIM_LOG-@LOG@}
if [ -z "$log" ]; then
	exec "$0.ref" "$@"
fi
mkdir -p "$log.err"
err=$(mktemp "$log.err/call.XXXXXXXX")
"$0.ref" "$@" 2> "$err"
status=$?
cat "$err" >&2
line="$status	$PWD	@SLOT@"
for a in "$@"; do line+="	$a"; done
printf '%s\n' "$line" >> "$log"
if [ -s "$err" ]; then
	mv -f "$err" "$log.err/$(printf '%s' "$line" | sha256sum | cut -c1-16)"
else
	rm -f "$err"
fi
exit $status
