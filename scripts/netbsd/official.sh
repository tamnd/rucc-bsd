#!/usr/bin/env bash
# Fetch the official binary sets of a release pin into the release directory (plan 12.8).
#
#   official.sh
#
# conf/netbsd/PIN.binary, beside the pin file PIN.pin of a release, names the binary sets of the
# release for amd64 in NB_PIN_BINARY_SETS, each with its SHA512 from the SHA512 file of the
# release, and their directory in NB_PIN_BINARY_URL. conf/netbsd/PIN-MACHINE.binary, such as
# 11.1-i386.binary, does the same for another machine, and is read in its place when NB_MACHINE
# is that machine. The file is not in the pin file, so that the
# tools caches keyed by the pin file stay. The project built these sets with GCC, so an image of
# them is the official userland. Each set goes to $NB_REL/$NB_MACHINE/binary/sets and is checked.
# A retry goes on from the bytes already on disk, as in fetch.sh. The kernel sets are not fetched,
# because R's kernel takes their place.
#
# The script prints the names of the sets on one line, for SETS of rb image.

. "$(dirname "$0")/common.sh"

pin=${NB_PIN_FILE:-trunk.pin}
binary=$NB_REPO/conf/netbsd/${pin%.pin}.binary
if [ "$NB_MACHINE" != amd64 ]; then
	binary=$NB_REPO/conf/netbsd/${pin%.pin}-$NB_MACHINE.binary
fi
if [ ! -f "$binary" ]; then
	echo "official.sh: the pin $NB_PIN_NAME has no binary sets, as there is no $binary" >&2
	exit 1
fi
# shellcheck source=../../conf/netbsd/11.1.binary
. "$binary"
dir=$NB_REL/$NB_MACHINE/binary/sets
mkdir -p "$dir"
names=()
for set in $NB_PIN_BINARY_SETS; do
	file=${set%%=*}
	curl -fsSL --retry 10 --retry-all-errors --retry-delay 2 -C - -o "$dir/$file" \
		"$NB_PIN_BINARY_URL/$file" >&2
	echo "${set#*=}  $dir/$file" | sha512sum -c - >&2
	# The sets of amd64 are .tar.xz and those of i386 are .tgz.
	name=${file%%.tar.*}
	names+=("${name%.tgz}")
done
echo "${names[*]}"
