#!/bin/sh
# Prepare the DragonFly builder VM, run as root in it.
#
#   provision.sh OUTDIR
#
# It makes a file system on the second disk, da1, at /build. The reference compiler is the
# base GCC of the builder's release, which needs no package. OUTDIR/toolchain.txt records the
# release, the default CCVER, what cc, gcc and ld say they are, the SHA-256 of the programs behind
# them, and /etc/compilers.conf if there is one.
set -eu
out=$1
mkdir -p "$out"

# UFS is the file system of the build disk. If newfs does not take the whole disk with no label,
# HAMMER2 is used, which does.
fs=
if ! mount | awk '$3 == "/build"' | grep -q .; then
	mkdir -p /build
	if newfs /dev/da1 > "$out/newfs.log" 2>&1 && mount -o noatime /dev/da1 /build; then
		fs=ufs
	else
		newfs_hammer2 -L BUILD /dev/da1 >> "$out/newfs.log" 2>&1
		mount_hammer2 /dev/da1@BUILD /build
		fs=hammer2
	fi
fi

# objformat(1) runs the compiler of CCVER from /usr/libexec/CCVER. With no CCVER it runs the
# default of the release, which make -V tells.
ccver=$(make -f /usr/share/mk/sys.mk -V CCVER 2>/dev/null || true)
{
	echo "# host"
	uname -a
	echo
	echo "# CCVER of sys.mk: ${ccver:-unset}"
	echo
	echo "# /build: ${fs:-mounted before} $(df -h /build | awk 'NR == 2 { print $2 }')"
	echo
	echo "# cc --version"
	cc --version | head -1
	echo
	echo "# gcc -v"
	gcc -v 2>&1 | tail -1
	echo
	echo "# ld --version"
	ld --version | head -1
	echo
	echo "# programs: sha256"
	for f in /usr/libexec/"${ccver:-gcc80}"/cc /usr/libexec/"${ccver:-gcc80}"/cc1 /usr/libexec/binutils*/elf/ld.bfd; do
		[ -f "$f" ] && echo "$(sha256 -q "$f") $f"
	done
	echo
	echo "# /usr/libexec"
	for f in /usr/libexec/gcc* /usr/libexec/binutils* /usr/libexec/custom; do
		[ -e "$f" ] && echo "$f"
	done
	echo
	echo "# /etc/compilers.conf"
	cat /etc/compilers.conf 2>/dev/null || echo "none"
} > "$out/toolchain.txt" 2>&1
cat "$out/toolchain.txt"
