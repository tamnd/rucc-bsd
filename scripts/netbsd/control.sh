#!/usr/bin/env bash
# Keep the control build of the world job, so that a later run of the same pin does not build it.
#
#   control.sh save FILE
#   control.sh restore FILE
#
# "save" runs after "overlay.sh ref" and "build.sh distribution" through that TOOLDIR. It keeps the
# nbmake wrapper of the control as $NB_ROOT/nbmake-r and the sizes of its boot code in
# $NB_LOGS/stand-ref.tsv (stand.sh ref). Then it removes the ELF files and the archives from the
# object directory and DESTDIR, because the replay compiles only and needs only the sources, the
# generated files and the headers. obj/tools is left as it is, because the tools cache holds it.
# FILE is then a tar compressed with zstd of the object directory without obj/tools, DESTDIR,
# nbmake-r and the shim log of the control with the standard error that shim-ref.sh kept.
#
# "restore" unpacks FILE into $NB_ROOT, and the job goes on as if it had built the control.

. "$(dirname "$0")/common.sh"

mode=$1
file=$2

case $mode in
save)
	cp "${NB_TOOLDIR_R:-$NB_ROOT/tools-r}/bin/nbmake-$NB_MACHINE" "$NB_ROOT/nbmake-r"
	"$NB_HERE/stand.sh" ref
	mkdir -p "$NB_LOGS/shim.tsv.err"
	python3 -I - "$NB_OBJ" "$NB_DEST" <<'PY'
import os
import sys

obj, dest = sys.argv[1:]
count = size = 0
for top in (obj, dest):
    for root, dirs, files in os.walk(top):
        if root == obj and "tools" in dirs:
            dirs.remove("tools")
        for name in files:
            path = os.path.join(root, name)
            if os.path.islink(path):
                continue
            with open(path, "rb") as f:
                head = f.read(8)
            if head[:4] == b"\x7fELF" or head == b"!<arch>\n":
                size += os.path.getsize(path)
                os.remove(path)
                count += 1
print(f"control: removed {count} ELF files and archives, {size >> 20} MiB")
PY
	mkdir -p "$(dirname "$file")"
	tar -C "$NB_ROOT" --exclude=./obj/tools -cf - ./obj ./destdir ./nbmake-r \
		./logs/shim.tsv ./logs/shim.tsv.err ./logs/stand-ref.tsv | zstd -q -T0 -3 -o "$file"
	echo "control: $file is $(($(stat -c %s "$file") >> 20)) MiB"
	;;
restore)
	zstd -q -dc "$file" | tar -C "$NB_ROOT" -xf -
	echo "control: restored $file"
	;;
*)
	echo "usage: control.sh save|restore FILE" >&2
	exit 2
	;;
esac
