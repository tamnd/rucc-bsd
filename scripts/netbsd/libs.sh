#!/usr/bin/env bash
# The libraries of a NetBSD release for rb abi-diff (plan 09.4).
#
#   libs.sh SETS OUT
#
# SETS is the sets directory of a release, such as release/amd64/binary/sets. The script extracts
# into OUT each shared object (*.so and *.so.*), each libc.a and the dynamic linkers ld.elf_so and
# ld.elf_so-i386 of lib, libexec, usr/lib and usr/libexec, as files and links. usr/lib/i386 has
# the compat -m32 libraries of the base32 and comp sets. The kernel, debug, modules and tests sets
# have none of these files and are left out. One line per set goes to standard output.
set -euo pipefail
sets=$1
out=$2
mkdir -p "$out"
list=$(mktemp)
trap 'rm -f "$list"' EXIT
for set in "$sets"/*.t*; do
	name=${set##*/}
	case $name in kern-* | debug* | modules.* | tests.*) continue ;; esac
	tar -tf "$set" | grep -E '^\./(lib|libexec|usr/lib|usr/libexec)/' | grep -v /debug/ \
		| grep -E '(\.so(\.[0-9.]+)?|/libc\.a|/ld\.elf_so(-i386)?)$' > "$list" || true
	if [ -s "$list" ]; then tar -xf "$set" -C "$out" -T "$list"; fi
	echo "$name: $(wc -l < "$list") files"
done
