#!/usr/bin/env bash
# Run a command in the NetBSD host container (provision/builders/netbsd-host).
#
#   run.sh COMMAND [ARGS...]
#
# The command runs as the calling user, in this repository, with NB_ROOT mounted at the same path,
# because a TOOLDIR records absolute paths. The NB_* variables and MKREPRO_TIMESTAMP pass through.
# NB_CONTAINER names the image, netbsd-host by default.
set -eu
here=$(cd "$(dirname "$0")/../../.." && pwd)
: "${NB_ROOT:=/mnt/nb}"
envs=()
while IFS='=' read -r k _; do
	case $k in NB_* | MKREPRO_TIMESTAMP) envs+=(-e "$k") ;; esac
done < <(env)
exec docker run --rm --init -u "$(id -u):$(id -g)" -e HOME=/tmp -e NB_ROOT="$NB_ROOT" "${envs[@]}" \
	-v "$NB_ROOT:$NB_ROOT" -v "$here:$here" -w "$here" "${NB_CONTAINER:-netbsd-host}" "$@"
