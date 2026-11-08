#!/usr/bin/env bash
# Make a copy of the live image whose boot loader and kernel use the serial console.
#
#   prepare-image.sh LIVE.img.gz OUT.img
#
# The boot options of the primary bootstrap are set to console=com0 with nbinstallboot -e, which
# edits a bootstrap that is already installed. This is how NetBSD makes the serial console variants
# of its own install images. No file in the image's file system changes, and the tree is not
# touched.
#
# The boot jobs then grow the disk with truncate. The image's rc.conf runs resize_gpt and
# resize_root at the first boot, which grow the root file system into the new space and reboot.

. "$(dirname "$0")/common.sh"

in=$1
out=$2
tooldir=${NB_TOOLDIR:-$NB_TOOLS}

gzip -dc "$in" > "$out"

# The root file system is the second GPT partition. Its first sectors hold bootxx_ffsv2, whose
# options installboot edits. Copy the first 64 KiB out, edit them and write them back.
start=$(sfdisk -J "$out" | python3 -c '
import json, sys
parts = json.load(sys.stdin)["partitiontable"]["partitions"]
print(parts[1]["start"])')
part=$(mktemp)
dd if="$out" of="$part" bs=512 skip="$start" count=128 status=none
"$tooldir/bin/nbinstallboot" -v -e -m "$NB_MACHINE" -o console=com0,speed=115200 "$part"
dd if="$part" of="$out" bs=512 seek="$start" count=128 conv=notrunc status=none
rm -f "$part"

echo "prepare-image: $out, root file system at sector $start, console com0 at 115200"
