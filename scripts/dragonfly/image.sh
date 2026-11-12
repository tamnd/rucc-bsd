#!/bin/sh
# Turn a DragonFly build into a bootable test image (plan 12.3), run as root in the builder VM.
#
#   image.sh
#
# Environment: RB_SRC, RB_OBJ, RB_OUT, RB_KERNCONF and RB_CCVER as for build.sh, and
#   RB_HARNESS   this repository, copied into the VM
#   RB_DESTDIR   the staging tree
#   RB_IMAGE     the raw image to write
#   RB_IMAGE_SIZE the size of the image, 8G by default
#
# The steps are those of the mkimg target of nrelease/Makefile, with no EFI slice:
# 1. installworld and installkernel into RB_DESTDIR from the objects of build.sh, and the
#    distribution target of etc, as nrelease does.
# 2. The image changes, made after the build and never in the tree: fstab, loader.conf with the
#    serial console, rc.conf, rb-rc, pkg and a local package repository.
# 3. An MBR with one slice, a disklabel with the label RBROOT and one UFS partition a, boot1_64 and
#    boot2_64, and a copy of the staging tree. The root is mounted by the label of the disklabel,
#    so the image boots on any disk controller. The ttys of x86_64 has a getty on ttyd0 when it
#    is the console, so the serial console gives a login.
set -eu
: "${RB_KERNCONF:=X86_64_GENERIC}"
: "${RB_CCVER:=}"
: "${RB_IMAGE_SIZE:=8G}"
export MAKEOBJDIRPREFIX="$RB_OBJ"
guest="$RB_HARNESS/scripts/dragonfly/guest"
d="$RB_DESTDIR"
work=$(dirname "$RB_IMAGE")
mnt="$work/mnt"
label=RBROOT
mkdir -p "$RB_OUT" "$work"

now() { date +%s; }
phase_done() {
	printf '%s\t0\t%s\t%s\t%s\n' "$1" "$(($(now) - $2))" "$(du -sk "$3" | cut -f1)" \
		"$(df -k "$RB_OBJ" | awk 'NR == 2 { print $3 }')" >> "$RB_OUT/phases.tsv"
}

# 1. Install.
start=$(now)
rm -rf "$d"
mkdir -p "$d"
: > "$RB_OUT/install.log"
for target in installworld installkernel; do
	# shellcheck disable=SC2086
	if ! env ${RB_CCVER:+WORLD_CCVER=$RB_CCVER} make -C "$RB_SRC" "$target" DESTDIR="$d" \
		KERNCONF="$RB_KERNCONF" __MAKE_CONF=/dev/null >> "$RB_OUT/install.log" 2>&1; then
		tail -60 "$RB_OUT/install.log"
		exit 1
	fi
done
if ! (cd "$RB_SRC/etc" && env MAKEOBJDIRPREFIX="$work/etc-obj" make -m "$RB_SRC/share/mk" \
	DESTDIR="$d" __MAKE_CONF=/dev/null distribution) >> "$RB_OUT/install.log" 2>&1; then
	tail -60 "$RB_OUT/install.log"
	exit 1
fi
rm -rf "$work/etc-obj"
phase_done install "$start" "$d"

# 2. The image changes.
start=$(now)
cat > "$d/etc/fstab" <<EOF
# Device			Mountpoint	FStype	Options		Dump	Pass#
/dev/part-by-label/$label.a	/		ufs	rw,noatime	1	1
proc				/proc		procfs	rw		0	0
EOF
cat >> "$d/boot/loader.conf" <<EOF
console="comconsole"
boot_serial="YES"
autoboot_delay="2"
vfs.root.mountfrom="ufs:part-by-label/$label.a"
EOF
cat >> "$d/etc/rc.conf" <<'EOF'
hostname="rb-dragonfly"
ifconfig_em0="DHCP"
rb_enable="YES"
EOF
install -m 555 "$guest/rb" "$d/etc/rc.d/rb"
# pkg, and the local repository for the smoke suite (plan 12.2). The pkg of the builder installs into
# the staging tree and fetches the packages of the repository, so the image needs no network.
export ASSUME_ALWAYS_YES=yes
if ! { pkg -r "$d" install -y pkg && pkg fetch -y -o "$d/var/db/rb-repo" tree &&
	pkg repo "$d/var/db/rb-repo"; } > "$RB_OUT/packages.log" 2>&1; then
	tail -30 "$RB_OUT/packages.log"
	exit 1
fi
mkdir -p "$d/usr/local/etc/rb-pkg"
cat > "$d/usr/local/etc/rb-pkg/rb.conf" <<'EOF'
rb: { url: "file:///var/db/rb-repo", signature_type: "none", enabled: yes }
EOF
pkg -r "$d" clean -ay >> "$RB_OUT/packages.log" 2>&1 || true
pkg -r "$d" query '%n %v' > "$RB_OUT/packages.txt"
phase_done overlay "$start" "$d"

# 3. The image.
start=$(now)
rm -f "$RB_IMAGE"
truncate -s "$RB_IMAGE_SIZE" "$RB_IMAGE"
vn=$(vnconfig -e vn "$RB_IMAGE")
trap 'umount "$mnt" 2>/dev/null || true; vnconfig -u "$vn" 2>/dev/null || true' EXIT
fdisk -b "$d/boot/mbr" -IB "$vn" > "$RB_OUT/fdisk.log" 2>&1
disklabel -w -r "${vn}s1" auto
disklabel -r "${vn}s1" > "$work/label"
echo "label: $label" >> "$work/label"
echo "a: * * 4.2BSD" >> "$work/label"
disklabel -R -r "${vn}s1" "$work/label"
rm -f "$work/label"
disklabel -B -b "$d/boot/boot1_64" -s "$d/boot/boot2_64" "${vn}s1"
newfs "/dev/${vn}s1a" > "$RB_OUT/newfs-image.log"
mkdir -p "$mnt"
mount "/dev/${vn}s1a" "$mnt"
cpdup -i0 -q "$d" "$mnt"
df -k "$mnt" | tail -1 > "$RB_OUT/image-df.txt"
umount "$mnt"
vnconfig -u "$vn"
trap - EXIT
# The base system has no zstd, so the host compresses the image as it copies it out.
printf 'image\t0\t%s\t%s\t%s\n' "$(($(now) - start))" "$(du -k "$RB_IMAGE" | cut -f1)" \
	"$(df -k "$RB_OBJ" | awk 'NR == 2 { print $3 }')" >> "$RB_OUT/phases.tsv"
{
	echo "raw_sha256 $(sha256 -q "$RB_IMAGE")"
	echo "raw_bytes $(stat -f %z "$RB_IMAGE")"
	echo "raw_allocated_kb $(du -k "$RB_IMAGE" | cut -f1)"
	echo "staging_kb $(du -sk "$d" | cut -f1)"
} > "$RB_OUT/image.txt"
cat "$RB_OUT/image.txt"
