#!/bin/sh
# Copy the kernel and the modules of one buildkernel aside, run as root in the builder VM.
#
#   kmods.sh KERNEL_OBJDIR OUT
#
# KERNEL_OBJDIR is the object directory of the kernel configuration, such as
# /build/obj/build/src/amd64.amd64/sys/GENERIC. OUT gets the file kernel and each module as
# NAME.ko, as installkernel puts them in /boot/kernel. The .ko.full and .ko.debug files are not
# taken. OUT is made empty first. rb ctf-diff pairs the files of two such directories by name, and
# rb kmod-relocs reads the modules.
set -eu
src=$1
out=$2
rm -rf "$out"
mkdir -p "$out"
cp "$src/kernel" "$out/kernel"
if [ -d "$src/modules" ]; then
	find "$src/modules" -type f -name '*.ko' -exec cp {} "$out/" \;
fi
echo "$out: the kernel and $(find "$out" -name '*.ko' | wc -l | tr -d ' ') modules, $(du -sk "$out" | cut -f1) KB"
