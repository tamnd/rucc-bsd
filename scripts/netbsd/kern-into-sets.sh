#!/usr/bin/env bash
# Put a kernel set in the place of kern-GENERIC in a release directory.
#
#   kern-into-sets.sh BINARY SET
#
# BINARY is the release's binary directory, with sets/ and kernel/. SET is a kernel set from
# kernset.sh, such as kern-GENERIC from the rucc kernel or kern-RB-TEST. The live image takes
# its /netbsd from kern-GENERIC (KERN_SET in distrib/amd64/liveimage/Makefile.liveimage), so
# this is how rb image gets another kernel. The checksum files of the sets are made to agree
# with the new set, and kernel/netbsd-GENERIC.gz, if there is one, gets the same kernel.

set -euo pipefail

binary=$1
set=$2
sets=$binary/sets
name=kern-GENERIC.tar.${set##*.tar.}

if [ "$(realpath "$set")" != "$(realpath -m "$sets/$name")" ]; then
	cp "$set" "$sets/$name"
fi
for sum in MD5 SHA512; do
	[ -f "$sets/$sum" ] || continue
	hash=$("$(echo "$sum" | tr '[:upper:]' '[:lower:]')sum" "$sets/$name" | cut -d ' ' -f 1)
	sed -i "s|^$sum ($name) = .*|$sum ($name) = $hash|" "$sets/$sum"
done
if [ -f "$binary/kernel/netbsd-GENERIC.gz" ]; then
	tar -xOf "$sets/$name" ./netbsd | gzip -9n > "$binary/kernel/netbsd-GENERIC.gz"
fi
echo "kern-into-sets: $name from $(basename "$set")"
