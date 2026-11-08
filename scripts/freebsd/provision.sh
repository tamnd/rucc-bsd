#!/bin/sh
# Prepare the builder VM, run as root in it.
#
#   provision.sh OUTDIR
#
# It makes a UFS file system on the second disk at /build, and installs the reference toolchain
# packages of conf/freebsd/builder.conf from the VM's default package repository. It records in
# OUTDIR/toolchain.txt the package versions and repository, the SHA-256 of each package file, the
# CROSS_TOOLCHAIN file the port installs, and what the compiler and linker say they are.
set -eu
out=$1
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../conf/freebsd/builder.conf"
mkdir -p "$out"

if ! mount -p | awk '$2 == "/build"' | grep -q .; then
	newfs -U -t /dev/vtbd1 > /dev/null
	mkdir -p /build
	mount -o noatime /dev/vtbd1 /build
fi

pkg -N > /dev/null 2>&1 || env ASSUME_ALWAYS_YES=yes pkg bootstrap > /dev/null
env ASSUME_ALWAYS_YES=yes pkg update -q
# shellcheck disable=SC2086
env ASSUME_ALWAYS_YES=yes pkg install -q -y $RB_TOOLCHAIN_PKGS

tc=/usr/local/share/toolchains/$RB_CROSS_TOOLCHAIN.mk
xcc=$(make -f "$tc" -V XCC)
xld=$(make -f "$tc" -V CROSS_BINUTILS_PREFIX)ld
{
	echo "# host"
	uname -a
	echo
	echo "# packages: name version repository origin"
	# shellcheck disable=SC2086
	pkg query '%n %v %R %o' $RB_TOOLCHAIN_PKGS
	echo
	echo "# package files: sha256"
	for p in $RB_TOOLCHAIN_PKGS; do
		for f in /var/cache/pkg/"$p"-"$(pkg query %v "$p")".pkg /var/cache/pkg/"$p"-"$(pkg query %v "$p")"~*.pkg; do
			[ -f "$f" ] && echo "$(sha256 -q "$f") $(basename "$f")"
		done
	done
	echo
	echo "# repository"
	pkg -vv | sed -n '/^Repositories:/,$p'
	echo
	echo "# $tc"
	cat "$tc"
	echo
	echo "# $xcc --version"
	"$xcc" --version | head -1
	echo
	echo "# $xld --version"
	"$xld" --version | head -1
} > "$out/toolchain.txt"
cat "$out/toolchain.txt"
