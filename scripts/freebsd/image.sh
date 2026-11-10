#!/bin/sh
# Turn a FreeBSD build into a bootable test image (plan 12.3), run as root in the builder VM.
#
#   image.sh
#
# Environment: RB_SRC, RB_OBJ, RB_SRCCONF, RB_TOOLCHAIN, RB_OUT and RB_JOBS as for build.sh, and
#   RB_HARNESS   this repository, copied into the VM
#   RB_DESTDIR   the staging tree
#   RB_IMAGE     the raw image to write
#
# The steps:
# 1. installworld, installkernel and distribution into RB_DESTDIR, from the objects of build.sh.
# 2. The image changes, made after the build and never in the tree: fstab, boot.config, the lines
#    of conf/freebsd/image, rb-rc, the test packages, and a local package repository.
# 3. makefs for the root file system, an ESP with loader.efi, and mkimg with pmbr, gptboot, the ESP,
#    swap and root. These are the tools and the layout of release/tools/vmimage.subr. A vm-image
#    target on main needs a pkgbase repository, so rb uses the tools directly (plan 12.3).
#
# Two builds of one pin must give the same image (plan 16.1). The time of the image is the time of
# the tree's Makefile, which is the commit time of a git pin and the build time of a release. makefs
# -T and mkimg -t use it, and SOURCE_DATE_EPOCH has it for pkg. boot/entropy comes from it, not from
# /dev/random. pkg's database gets it as the install time of each package. RB_OUT gets
# install.manifest after step 1 and staging.manifest after step 2: one line per file with its mode,
# owner, size and SHA-256, and one line per link with its target. A second build compares them.
set -eu
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
: "${RB_IMAGE_SIZE:=10g}"
export MAKEOBJDIRPREFIX="$RB_OBJ"
conf="$RB_HARNESS/conf/freebsd/image"
guest="$RB_HARNESS/scripts/freebsd/guest"
d="$RB_DESTDIR"
work=$(dirname "$RB_IMAGE")
mkdir -p "$RB_OUT" "$work"
# A kernel configuration of rucc-bsd, such as RB-TEST (plan 12.2), is in conf/freebsd/kernel of
# the harness. make finds it by KERNCONFDIR, and the configurations of the tree in sys/amd64/conf.
kernconfdir=
if [ -f "${RB_HARNESS:-}/conf/freebsd/kernel/${RB_KERNCONF:-GENERIC}" ]; then
	kernconfdir="$RB_HARNESS/conf/freebsd/kernel"
fi

now() { date +%s; }
epoch=$(stat -f %m "$RB_SRC/Makefile")
export SOURCE_DATE_EPOCH="$epoch"
manifest() {
	(
		cd "$1" || exit 1
		find . -print0 | sort -z | xargs -0 stat -f '%Sp %Su %Sg %z %N%SY'
		find . -type f -print0 | sort -z | xargs -0 sha256 -r
	) > "$2" || echo "the manifest of $1 is not complete" >&2
}
phase_done() {
	printf '%s\t0\t%s\t%s\t%s\n' "$1" "$(($(now) - $2))" "$(du -sk "$3" | cut -f1)" \
		"$(df -k "$RB_OBJ" | awk 'NR == 2 { print $3 }')" >> "$RB_OUT/phases.tsv"
}

# 1. Install.
start=$(now)
rm -rf "$d"
mkdir -p "$d"
if ! make -C "$RB_SRC" -j"$RB_JOBS" installworld installkernel distribution DESTDIR="$d" \
	TARGET=amd64 TARGET_ARCH=amd64 KERNCONF="${RB_KERNCONF:-GENERIC}" ${kernconfdir:+KERNCONFDIR="$kernconfdir"} \
	CROSS_TOOLCHAIN="$RB_TOOLCHAIN" __MAKE_CONF=/dev/null SRCCONF="$RB_SRCCONF" \
	> "$RB_OUT/install.log" 2>&1; then
	tail -60 "$RB_OUT/install.log"
	exit 1
fi
manifest "$d" "$RB_OUT/install.manifest"
phase_done install "$start" "$d"

# 2. The image changes.
start=$(now)
cat > "$d/etc/fstab" <<'EOF'
# Device		Mountpoint	FStype	Options	Dump	Pass#
/dev/gpt/rootfs		/		ufs	rw	1	1
/dev/gpt/swapfs		none		swap	sw	0	0
fdesc			/dev/fd		fdescfs	rw	0	0
EOF
cat "$conf/loader.conf" >> "$d/boot/loader.conf"
cat "$conf/rc.conf" >> "$d/etc/rc.conf"
cat "$conf/sysctl.conf" >> "$d/etc/sysctl.conf"
mkdir -p "$d/etc/kyua"
[ -s "$d/etc/kyua/kyua.conf" ] || echo 'syntax(2)' > "$d/etc/kyua/kyua.conf"
cat "$conf/kyua.conf" >> "$d/etc/kyua/kyua.conf"
# rb-rc is in the image. The suites and the kyua exclusions come on the rb disk of each boot.
install -m 555 "$guest/rb" "$d/etc/rc.d/rb"
# boot2 and the BIOS loader use the serial console too, so a BIOS boot that stops early shows why.
echo '-S115200 -Dh' > "$d/boot.config"
for i in $(jot 32); do sha512 -q -s "rb-entropy $epoch $i"; done | head -c 4096 > "$d/boot/entropy"

# The test packages, installed with the image's own pkg in a chroot, as freebsd-ci does.
cp /etc/resolv.conf "$d/etc/resolv.conf"
mount -t devfs devfs "$d/dev"
trap 'umount "$d/dev" 2>/dev/null || true' EXIT
pkgs=$(sed -e 's/#.*//' "$conf/packages" | xargs)
smoke_pkgs=$(sed -e 's/#.*//' "$conf/smoke-packages" | xargs)
chroot "$d" env ASSUME_ALWAYS_YES=yes IGNORE_OSVERSION=yes pkg bootstrap -f > "$RB_OUT/packages.log" 2>&1
# A package the repository lacks today must not stop the image. Its tests are then skipped, and
# packages.txt shows that it is missing.
# shellcheck disable=SC2086
if ! chroot "$d" env ASSUME_ALWAYS_YES=yes IGNORE_OSVERSION=yes pkg install -y $pkgs >> "$RB_OUT/packages.log" 2>&1; then
	for p in $pkgs; do
		chroot "$d" env ASSUME_ALWAYS_YES=yes IGNORE_OSVERSION=yes pkg install -y "$p" >> "$RB_OUT/packages.log" 2>&1 ||
			echo "missing $p" >> "$RB_OUT/packages-missing.txt"
	done
fi
chroot "$d" pkg query '%n %v %R' | sort > "$RB_OUT/packages.txt"
# The local repository for the smoke suite (plan 12.2).
# shellcheck disable=SC2086
chroot "$d" env ASSUME_ALWAYS_YES=yes IGNORE_OSVERSION=yes pkg fetch -y -o /var/db/rb-repo $smoke_pkgs \
	>> "$RB_OUT/packages.log" 2>&1
chroot "$d" pkg repo /var/db/rb-repo >> "$RB_OUT/packages.log" 2>&1
mkdir -p "$d/usr/local/etc/rb-pkg"
cat > "$d/usr/local/etc/rb-pkg/rb.conf" <<'EOF'
rb: { url: "file:///var/db/rb-repo", signature_type: "none", enabled: yes }
EOF
chroot "$d" pkg clean -ay >> "$RB_OUT/packages.log" 2>&1
# pkg shell opens the local database when it has no file argument, and reads SQL on its input.
echo "UPDATE packages SET time = $epoch; VACUUM;" | chroot "$d" pkg shell >> "$RB_OUT/packages.log" 2>&1 ||
	echo "pkg shell failed, so the install times stay" >> "$RB_OUT/packages.log"
chroot "$d" pkg query '%t' | sort | uniq -c >> "$RB_OUT/packages.log"
# A package script that runs pw adds a line to /var/log/userlog that starts with the wall-clock
# time. That time becomes the image's time, as for pkg's database.
if [ -f "$d/var/log/userlog" ]; then
	stamp=$(date -u -r "$epoch" '+%Y-%m-%d %H:%M:%S')
	sed -i '' -E "s/^[0-9]{4}-[0-9]{2}-[0-9]{2} [0-9]{2}:[0-9]{2}:[0-9]{2}/$stamp/" "$d/var/log/userlog"
fi
umount "$d/dev"
trap - EXIT
rm -f "$d/etc/resolv.conf"
manifest "$d" "$RB_OUT/staging.manifest"
phase_done overlay "$start" "$d"

# 3. The image.
start=$(now)
rm -rf "$work/esp" "$work/esp.img" "$work/root.ufs" "$RB_IMAGE"
mkdir -p "$work/esp/EFI/BOOT"
cp "$d/boot/loader.efi" "$work/esp/EFI/BOOT/bootx64.efi"
makefs -T "$epoch" -t msdos -o volume_id=0x52420001 -o fat_type=32 -o sectors_per_cluster=1 \
	-o volume_label=EFISYS -s 64m \
	"$work/esp.img" "$work/esp" > /dev/null
makefs -T "$epoch" -B little -t ffs -Z -s "$RB_IMAGE_SIZE" -f 200000 \
	-o label=rootfs,version=2,softupdates=1,bsize=32768,fsize=4096 \
	"$work/root.ufs" "$d" > "$RB_OUT/makefs.log"
esp_sha256=$(sha256 -q "$work/esp.img")
root_sha256=$(sha256 -q "$work/root.ufs")
mkimg -t "$epoch" -s gpt -f raw \
	-b "$d/boot/pmbr" \
	-p freebsd-boot/bootfs:="$d/boot/gptboot" \
	-p efi/efiboot0:="$work/esp.img" \
	-p freebsd-swap/swapfs::1G \
	-p freebsd-ufs/rootfs:="$work/root.ufs" \
	-o "$RB_IMAGE"
rm -f "$work/root.ufs" "$work/esp.img"
zstd -q -T0 -f -9 "$RB_IMAGE" -o "$RB_IMAGE.zst"
printf 'image\t0\t%s\t%s\t%s\n' "$(($(now) - start))" "$(du -k "$RB_IMAGE.zst" | cut -f1)" \
	"$(df -k "$RB_OBJ" | awk 'NR == 2 { print $3 }')" >> "$RB_OUT/phases.tsv"
{
	echo "epoch $epoch"
	echo "esp_sha256 $esp_sha256"
	echo "root_sha256 $root_sha256"
	echo "raw_sha256 $(sha256 -q "$RB_IMAGE")"
	echo "raw_bytes $(stat -f %z "$RB_IMAGE")"
	echo "raw_allocated_kb $(du -k "$RB_IMAGE" | cut -f1)"
	echo "zst_bytes $(stat -f %z "$RB_IMAGE.zst")"
	echo "zst_sha256 $(sha256 -q "$RB_IMAGE.zst")"
	echo "staging_kb $(du -sk "$d" | cut -f1)"
} > "$RB_OUT/image.txt"
cat "$RB_OUT/image.txt"
