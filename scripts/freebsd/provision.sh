#!/bin/sh
# Prepare the builder VM, run as root in it.
#
#   provision.sh OUTDIR
#
# It makes a UFS file system at /build on the disk RB_BUILD_DEV, /dev/vtbd1 by default, and
# installs the reference toolchain packages of conf/freebsd/builder.conf from the VM's default
# package repository. With RB_TARGET_ARCH, such as aarch64, it installs the flavor of the same
# ports for that TARGET_ARCH, aarch64-gcc15 in place of amd64-gcc15, as rb toolchain does for the row. With RB_GCC=14 it
# installs gcc14 in place of gcc15, the fallback of personas.toml. It records in
# OUTDIR/toolchain.txt the package versions and repository, the SHA-256 of each package file, the
# CROSS_TOOLCHAIN file the port installs, and what the compiler and linker say they are.
set -eu
out=$1
here=$(cd "$(dirname "$0")" && pwd)
. "$here/../../conf/freebsd/builder.conf"
mkdir -p "$out"
arch=${RB_TARGET_ARCH:-amd64}
RB_TOOLCHAIN_PKGS=$(echo "$RB_TOOLCHAIN_PKGS" | sed "s/amd64-/$arch-/g")
RB_CROSS_TOOLCHAIN=$(echo "$RB_CROSS_TOOLCHAIN" | sed "s/^amd64-/$arch-/")
dev=${RB_BUILD_DEV:-/dev/vtbd1}
gcc=${RB_GCC:-15}
case $gcc in
14 | 15) ;;
*) echo "provision.sh: RB_GCC is 14 or 15, not $gcc" >&2; exit 2 ;;
esac
RB_TOOLCHAIN_PKGS=$(echo "$RB_TOOLCHAIN_PKGS" | sed "s/-gcc15/-gcc$gcc/g")
RB_CROSS_TOOLCHAIN=$(echo "$RB_CROSS_TOOLCHAIN" | sed "s/-gcc15\$/-gcc$gcc/")

if ! mount -p | awk '$2 == "/build"' | grep -q .; then
	newfs -U -t "$dev" > /dev/null
	mkdir -p /build
	mount -o noatime "$dev" /build
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
		f=/var/cache/pkg/"$p"-"$(pkg query %v "$p")".pkg
		if [ -f "$f" ]; then
			echo "$(sha256 -q "$f") $(basename "$f")"
		fi
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
