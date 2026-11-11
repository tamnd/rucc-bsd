#!/usr/bin/env bash
# Put the rucc kernel into a raw NetBSD release image of conf/tcg/images.conf (plan 12.6).
#
#   r-image.sh IMAGE KERNEL
#
# IMAGE is arm64.img or riscv64.img, unpacked, and the script changes it. KERNEL is the netbsd
# of the rucc leg of r-kernel.sh. distrib/utils/embedded/mkimage gives these images a GPT with two
# partitions: the EFI system partition, labelled EFI, and the root file system, labelled
# netbsd-root, which has the G kernel as /netbsd.
#
# Linux cannot write FFS, so the kernel goes into the EFI system partition as /netbsd, with
# mtools. Beside it goes /boot.cfg. efiboot reads boot.cfg from the partition that it was loaded
# from, which is fs0 when the image is the only disk with a file system that the firmware knows.
# The one menu entry of the file boots fs0:netbsd with root=NAME=netbsd-root, so the R kernel
# mounts the G world. The modules in /stand stay those of G.
#
# efiboot shows the banner line of boot.cfg, R_BANNER, on the console of each boot. When efiboot
# cannot load fs0:netbsd it stops at its prompt and the boot fails, so a boot that shows the
# banner and comes up runs the R kernel.
set -euo pipefail

image=$1
kernel=$2
R_BANNER=${R_BANNER:-rb R image: the rucc kernel fs0:netbsd with the G world}

# The byte offset of the partition labelled EFI, from the primary GPT. sfdisk reads the MBR of
# arm64.img, a hybrid MBR, and not the GPT.
offset=$(python3 -I - "$image" <<'EOF'
import struct, sys
with open(sys.argv[1], "rb") as f:
    f.seek(512)
    head = f.read(92)
    if head[:8] != b"EFI PART":
        sys.exit("r-image: the image has no GPT")
    lba, count, size = struct.unpack_from("<QII", head, 72)
    f.seek(lba * 512)
    table = f.read(count * size)
for i in range(count):
    entry = table[i * size:(i + 1) * size]
    if entry[56:128].decode("utf-16-le").rstrip("\0") == "EFI":
        print(struct.unpack_from("<Q", entry, 32)[0] * 512)
        break
else:
    sys.exit("r-image: the GPT has no partition labelled EFI")
EOF
)

export MTOOLS_SKIP_CHECK=1
esp="$image@@$offset"
cfg=$(mktemp)
trap 'rm -f "$cfg"' EXIT
printf '%s\n' "banner=$R_BANNER" \
	'menu=Boot the rucc kernel:boot fs0:netbsd root=NAME=netbsd-root' \
	'default=1' 'timeout=0' > "$cfg"
mcopy -o -i "$esp" "$kernel" ::/netbsd
mcopy -o -i "$esp" "$cfg" ::/boot.cfg
mdir -i "$esp" ::/
echo "r-image: $(basename "$kernel") into the EFI system partition of $(basename "$image"), at byte $offset"
