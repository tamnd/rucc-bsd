#!/usr/bin/env bash
# Fetch, check, unpack and grow one release image of conf/tcg/images.conf.
#
#   image.sh SYSTEM-ROW DIR SIZE
#
# It prints the path of the image in DIR. A qcow2 image stays qcow2 and a raw image stays raw, and
# both grow to SIZE, such as 16G, so that the suites have room. The systems grow their root file
# system to the disk at the first boot.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
name=$1
dir=$2
size=$3

line="$(awk -v n="$name" '$1 == n' "$here/../../conf/tcg/images.conf")"
if [ -z "$line" ]; then
	echo "no image $name in conf/tcg/images.conf" >&2
	exit 2
fi
read -r _ url algorithm digest <<< "$line"
mkdir -p "$dir"
file="$dir/$(basename "$url")"
curl -fsSL --retry 5 --retry-delay 10 -o "$file" "$url"
echo "$digest  $file" | "${algorithm}sum" -c --quiet - >&2

case $file in
*.xz)
	xz -d "$file"
	file=${file%.xz}
	;;
*.gz)
	gzip -d "$file"
	file=${file%.gz}
	;;
esac
case $file in
*.qcow2) qemu-img resize -q "$file" "$size" ;;
*) truncate -s "$size" "$file" ;;
esac
echo "$file"
