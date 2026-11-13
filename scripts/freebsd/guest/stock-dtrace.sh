#!/bin/sh
# The dtrace of the fixed point of B10 (plan 16.11), given to the build as DTRACE.
#
# dtrace -G writes the utsname of the host into the DOF of the program. The fixed job gives the VM
# the host name of R, and this script gives dtrace the kernel version of R by UNAME_v, which uname(3)
# reads. The fixed job writes the version to /build/stock/uname-v. The src Makefile sets PATH to
# /sbin:/bin:/usr/sbin:/usr/bin. dtrace gives cpp only its base name as argv[0], and the cpp of
# gcc15 finds cc1 from the place of argv[0] in PATH, so the script puts /usr/local/bin first.
set -eu
UNAME_v=$(cat /build/stock/uname-v)
PATH=/usr/local/bin:/sbin:/bin:/usr/sbin:/usr/bin
export UNAME_v PATH
exec dtrace "$@"
