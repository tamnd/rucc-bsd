#!/usr/bin/env bash
# Install Ubuntu packages on a runner, and try again when the mirror cannot be reached.
#
#   apt-install.sh PACKAGE...
#
# apt-get tries each download 5 times. When the whole update and install still fails, for example
# because the runner cannot resolve the name of the mirror for a minute, the script waits 20 s and
# runs both again, 3 times in all.
set -euo pipefail

if [ "$#" -eq 0 ]; then
	echo "usage: apt-install.sh PACKAGE..." >&2
	exit 2
fi

tries=3
for try in $(seq 1 "$tries"); do
	if sudo apt-get -o Acquire::Retries=5 update -q &&
		sudo apt-get -o Acquire::Retries=5 install -qy --no-install-recommends "$@"; then
		exit 0
	fi
	if [ "$try" -lt "$tries" ]; then
		echo "apt-install: try $try of $tries failed, trying again in 20 s" >&2
		sleep 20
	fi
done
echo "apt-install: the packages are not installed after $tries tries: $*" >&2
exit 1
