#!/usr/bin/env bash
# Fetch, check, unpack and grow one release image of conf/tcg/images.conf.
#
#   image.sh SYSTEM-ROW DIR SIZE
#
# It prints the path of the image in DIR. A qcow2 image stays qcow2 and a raw image stays raw, and
# both grow to SIZE, such as 16G, so that the suites have room. The systems grow their root file
# system to the disk at the first boot.
#
# A download that the server closes early goes on from where it stopped, 5 times in all, with
# waits of 20, 40, 60 and 120 s. curl's own --retry does not try again after a closed transfer
# (exit 18), and in the tcg nightly 43134416424 cdn.netbsd.org closed both NetBSD downloads.
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
waits=(20 40 60 120)
tries=$((${#waits[@]} + 1))
for try in $(seq 1 "$tries"); do
	if curl -fsSL --retry 5 --retry-delay 10 -C - -o "$file" "$url"; then
		break
	fi
	if [ "$try" -eq "$tries" ]; then
		echo "image.sh: $url could not be downloaded in $tries tries" >&2
		exit 1
	fi
	wait=${waits[$((try - 1))]}
	echo "image.sh: try $try of $tries failed, going on in $wait s" >&2
	sleep "$wait"
done
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
