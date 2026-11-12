#!/bin/sh
# HOST_CC of the R-host build (milestone B12): run rucc as the host C compiler of build.sh tools.
#
#   HOST_CC=scripts/netbsd/host-cc.sh build.sh tools
#
# NB_HOST_CC_RUCC is the rucc binary. Each call appends one line to NB_HOST_CC_LOG, when that is
# set: the directory and the arguments. So the log tells which host tools rucc compiled.
if [ -n "${NB_HOST_CC_LOG:-}" ]; then
	printf '%s\t%s\n' "$PWD" "$*" >> "$NB_HOST_CC_LOG"
fi
exec "${NB_HOST_CC_RUCC:-rucc}" "$@"
