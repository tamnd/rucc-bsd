#!/usr/bin/env bash
# Install Ubuntu packages on a runner, and try again when the mirror cannot be reached.
#
#   apt-install.sh PACKAGE...
#
# apt-get tries each download 5 times. When the whole update and install still fails, the script
# waits and runs both again, 5 times in all, with waits of 20, 40, 60 and 120 s between the tries.
#
# After the first failed try the runner gets its packages from archive.ubuntu.com and not from
# azure.archive.ubuntu.com. A runner image lists the Azure mirror first, in
# /etc/apt/apt-mirrors.txt for the mirror+file: sources and in the sources files, and in tcg run
# 43032296464 runners could not resolve its name for about 13 minutes.
set -euo pipefail

if [ "$#" -eq 0 ]; then
	echo "usage: apt-install.sh PACKAGE..." >&2
	exit 2
fi

waits=(20 40 60 120)
tries=$((${#waits[@]} + 1))
for try in $(seq 1 "$tries"); do
	if sudo apt-get -o Acquire::Retries=5 update -q &&
		sudo apt-get -o Acquire::Retries=5 install -qy --no-install-recommends "$@"; then
		exit 0
	fi
	if [ "$try" -eq "$tries" ]; then
		break
	fi
	if [ "$try" -eq 1 ]; then
		for f in /etc/apt/apt-mirrors.txt /etc/apt/sources.list.d/ubuntu.sources /etc/apt/sources.list; do
			if [ -f "$f" ]; then
				sudo sed -i 's/azure\.archive\.ubuntu\.com/archive.ubuntu.com/g' "$f"
			fi
		done
		echo "apt-install: the sources now name archive.ubuntu.com and not azure.archive.ubuntu.com" >&2
	fi
	wait=${waits[$((try - 1))]}
	echo "apt-install: try $try of $tries failed, trying again in $wait s" >&2
	sleep "$wait"
done
echo "apt-install: the packages are not installed after $tries tries: $*" >&2
exit 1
