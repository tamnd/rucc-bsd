#!/bin/sh
# The linker that dtrace -G runs in the fixed point of B10 (plan 16.11), given by -x ldpath.
#
# dtrace -G runs "LD -o FILE -r TFILE DRTI". DRTI is drti.o in the first library directory of
# dtrace, which is always /usr/lib/dtrace. In the RR image that drti.o is the one that R built,
# and the ld on PATH is the ld of binutils. R ran in the stock image, so it had the stock drti.o
# and the stock lld. This script gives RR the same inputs: it gives the stock drti.o in place of
# /usr/lib/dtrace/drti.o and runs the stock lld. The fixed job puts both in /build/stock.
set -eu
stock=/build/stock
for arg; do
	shift
	case $arg in
	/usr/lib/dtrace/drti.o) arg=$stock/drti.o ;;
	esac
	set -- "$@" "$arg"
done
LD_LIBRARY_PATH=$stock/lld exec "$stock/lld/ld.lld" "$@"
