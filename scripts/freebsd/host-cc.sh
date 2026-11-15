#!/bin/sh
# The host CC of the R-host build of FreeBSD (milestone B12), run in the builder VM.
#
#   CC=WRAPPER make buildworld buildkernel ...
#
# RB_HOST_CC_RUCC is the rucc binary. Each call appends one line to RB_HOST_CC_LOG, when that is
# set: the directory, the arguments and the compiler that ran, rucc or clang. So the log tells which
# host tools rucc compiled. make runs up to RB_JOBS calls at a time, so lockf holds the log while
# one line is written, and a long line is not mixed with another. freebsd-g puts the two variables
# in a small WRAPPER that runs this script, as netbsd-host does for scripts/netbsd/host-cc.sh.
#
# libdwarf_rw.c stays on clang (RB_HOST_CC_CLANG, /usr/bin/cc by default). Its sleb128 readers,
# _dwarf_read_sleb128 and _dwarf_decode_sleb128, sign-extend with ret |= (-1 << shift), and shift is
# 35 for a value of 32 bits, so the int shift goes past 31. That is undefined behavior. ctfconvert
# reads the enum constants of the kernel with it, and 1 << 31 (IB_QP_CREATE_RESERVED_END and
# IB_SEND_RESERVED_END of the OFED headers) gives -8 with rucc, with gcc and with clang -O0. Only
# clang -O2 gives the true value, -2147483648, and only by chance: it makes the shift -128 << 28.
# With rucc the .SUNW_ctf of the kernel and the OFED modules differs from the G-host build, so the
# file is built with clang and the pin is not patched.
cc=${RB_HOST_CC_RUCC:-rucc}
name=rucc
case " $* " in
*/libdwarf_rw.c\ * | *\ libdwarf_rw.c\ *)
	cc=${RB_HOST_CC_CLANG:-/usr/bin/cc}
	name=clang
	;;
esac
if [ -n "${RB_HOST_CC_LOG:-}" ]; then
	# shellcheck disable=SC2016
	/usr/bin/lockf -k "$RB_HOST_CC_LOG" /bin/sh -c 'printf "%s\t%s\t%s\n" "$1" "$2" "$3" >> "$4"' sh "$PWD" "$*" "$name" "$RB_HOST_CC_LOG"
fi
exec "$cc" "$@"
