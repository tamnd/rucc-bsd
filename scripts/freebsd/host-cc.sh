#!/bin/sh
# The host CC of the R-host build of FreeBSD (milestone B12), run in the builder VM.
#
#   CC=WRAPPER make buildworld buildkernel ...
#
# RB_HOST_CC_RUCC is the rucc binary. Each call appends one line to RB_HOST_CC_LOG, when that is
# set: the directory and the arguments. So the log tells which host tools rucc compiled. make runs
# up to RB_JOBS calls at a time, so lockf holds the log while one line is written, and a long line
# is not mixed with another. freebsd-g puts the two variables in a small WRAPPER that runs this
# script, as netbsd-host does for scripts/netbsd/host-cc.sh.
if [ -n "${RB_HOST_CC_LOG:-}" ]; then
	# shellcheck disable=SC2016
	/usr/bin/lockf -k "$RB_HOST_CC_LOG" /bin/sh -c 'printf "%s\t%s\n" "$1" "$2" >> "$3"' sh "$PWD" "$*" "$RB_HOST_CC_LOG"
fi
exec "${RB_HOST_CC_RUCC:-rucc}" "$@"
