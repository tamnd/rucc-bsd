#!/bin/sh
# The libraries of a FreeBSD tree for rb abi-diff (plan 09.4), run as root in the builder VM.
#
#   libs.sh ROOT OUT.tar.xz
#   libs.sh --image IMAGE.zst OUT.tar.xz
#
# ROOT is a staging tree, such as RB_DESTDIR of image.sh. With --image, the script uses the root
# file system of a GPT image instead, such as freebsd.img.zst of a freebsd-g run. It attaches a
# copy of the image with mdconfig and mounts the file system read-only.
#
# The archive has each shared object (*.so and *.so.*), ld-elf.so.1, ld-elf32.so.1 and each libc.a
# of lib, usr/lib, usr/lib32 and libexec, as files and links. It has no debug file.
set -eu
if [ "$1" = --image ]; then
	img=$2
	out=$3
	raw="${img%.zst}.libs.raw"
	mnt="$(dirname "$out")/libs-mnt"
	mkdir -p "$mnt"
	zstd -d -q -f --sparse "$img" -o "$raw"
	md=$(mdconfig -a -t vnode -o readonly -f "$raw")
	trap 'umount "$mnt" 2>/dev/null; mdconfig -d -u "$md" 2>/dev/null; rm -f "$raw"' EXIT
	part=$(gpart show -p "$md" | awk '$4 == "freebsd-ufs" { print $3; exit }')
	mount -o ro "/dev/$part" "$mnt"
	root=$mnt
else
	root=$1
	out=$2
fi
(
	cd "$root"
	dirs=
	for d in lib usr/lib usr/lib32 libexec; do
		[ -d "$d" ] && dirs="$dirs $d"
	done
	# shellcheck disable=SC2086
	find $dirs -path usr/lib/debug -prune -o \( -name '*.so' -o -name '*.so.*' -o -name libc.a \) \
		\( -type f -o -type l \) -print | sort > "$out.list"
	tar -cf - -T "$out.list" | xz -T0 > "$out"
)
echo "$(wc -l < "$out.list" | tr -d ' ') files from $root in $out, $(du -k "$out" | cut -f1) KB"
rm -f "$out.list"
