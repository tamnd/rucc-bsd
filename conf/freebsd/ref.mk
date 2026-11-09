# The CROSS_TOOLCHAIN file of G for FreeBSD 15.1 on amd64 (plan 05.4). Pass its path:
# make buildworld buildkernel CROSS_TOOLCHAIN=/path/to/ref.mk RB_SHIMS=/path/to/bin
# RB_SHIMS is the directory that rb toolchain or rb shim filled. rucc.mk differs in XCC only.
# rb toolchain-diff checks this, and a test checks that rb toolchain writes the same lines.
RB_SHIMS?=/usr/local/rb/bin
XCC=${RB_SHIMS}/x86_64-unknown-freebsd15.1-rb-cc-ref
XCXX=${RB_SHIMS}/x86_64-unknown-freebsd15.1-rb-c++
XCPP=${RB_SHIMS}/x86_64-unknown-freebsd15.1-rb-cpp
CROSS_BINUTILS_PREFIX=/usr/local/x86_64-unknown-freebsd15.1/bin/
X_COMPILER_TYPE=gcc
