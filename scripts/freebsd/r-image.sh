#!/bin/sh
# Put the kernel of this build into the image of a G run, run as root in the builder VM.
#
#   r-image.sh G_IMAGE_ZST
#
# Environment: RB_SRC, RB_OBJ, RB_SRCCONF, RB_TOOLCHAIN, RB_KERNCONF and RB_OUT as for build.sh,
# and RB_IMAGE, the raw image to write. RB_IMAGE.zst is the result.
#
# G_IMAGE_ZST is freebsd.img.zst of a freebsd-g run: G world, G kernel and the test packages. The
# script attaches a copy of it with mdconfig, mounts its root file system, removes the G kernel and
# its debug files, and runs installkernel into it with RB_TOOLCHAIN. So the image has the world of
# G and the kernel and modules of the last buildkernel in RB_OBJ. It writes "r-image" to
# phases.tsv and the sizes to r-image.txt.
set -eu
g=$1
export MAKEOBJDIRPREFIX="$RB_OBJ"
work=$(dirname "$RB_IMAGE")
mnt="$work/mnt"
mkdir -p "$RB_OUT" "$work" "$mnt"
start=$(date +%s)

zstd -d -q -f --sparse "$g" -o "$RB_IMAGE"
md=$(mdconfig -a -t vnode -f "$RB_IMAGE")
trap 'umount "$mnt" 2>/dev/null; mdconfig -d -u "$md" 2>/dev/null' EXIT
part=$(gpart show -p "$md" | awk '$4 == "freebsd-ufs" { print $3; exit }')
mount "/dev/$part" "$mnt"
rm -rf "$mnt/boot/kernel" "$mnt/boot/kernel.old" "$mnt/usr/lib/debug/boot/kernel"
if ! make -C "$RB_SRC" installkernel DESTDIR="$mnt" TARGET=amd64 TARGET_ARCH=amd64 \
	KERNCONF="${RB_KERNCONF:-GENERIC}" CROSS_TOOLCHAIN="$RB_TOOLCHAIN" __MAKE_CONF=/dev/null \
	SRCCONF="$RB_SRCCONF" > "$RB_OUT/r-install.log" 2>&1; then
	tail -60 "$RB_OUT/r-install.log"
	exit 1
fi
df -k "$mnt" > "$RB_OUT/r-image-df.txt"
umount "$mnt"
mdconfig -d -u "$md"
trap - EXIT

zstd -q -T0 -f -9 "$RB_IMAGE" -o "$RB_IMAGE.zst"
printf 'r-image\t0\t%s\t%s\t%s\n' "$(($(date +%s) - start))" "$(du -k "$RB_IMAGE.zst" | cut -f1)" \
	"$(df -k "$RB_OBJ" | awk 'NR == 2 { print $3 }')" >> "$RB_OUT/phases.tsv"
{
	echo "raw_bytes $(stat -f %z "$RB_IMAGE")"
	echo "raw_allocated_kb $(du -k "$RB_IMAGE" | cut -f1)"
	echo "zst_bytes $(stat -f %z "$RB_IMAGE.zst")"
	echo "zst_sha256 $(sha256 -q "$RB_IMAGE.zst")"
	echo "toolchain $RB_TOOLCHAIN"
} > "$RB_OUT/r-image.txt"
cat "$RB_OUT/r-image.txt"
