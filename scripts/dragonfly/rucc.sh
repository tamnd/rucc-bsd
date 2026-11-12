#!/bin/sh
# Build rucc natively in the DragonFly builder VM and install it as a CCVER compiler through
# /etc/compilers.conf (plan 05.3 and 16.12). Run as root in the VM after provision.sh.
#
#   rucc.sh OUTDIR CCVER
#
# The source of rucc is in /build/rucc-src, copied in by the host. rucc has no dependencies from
# crates.io, so cargo needs no network. Rust is the rust package of the DragonFly package
# repository, as rustup has no host toolchain for DragonFly. /usr/local of the live system is on
# its root disk, which has little free space, so the packages go to /build/local, which is mounted
# on /usr/local with mount_null.
#
# compilers.conf gets CCVER_CC, CCVER_GCC and CCVER_CPP for rucc. C++ stays with the reference
# g++ of the release, as in the R builds of FreeBSD and NetBSD. The include options are those of
# the base compilers: STD_INCOPT of /etc/defaults/compilers.conf. In the world stage of buildworld,
# the cc script of /usr/libexec/custom sets INCPREFIX to the world tree, so --sysroot of the same
# value makes rucc link with the crt files and libraries of the world tree, as the gcc80 of the
# cross-tools does. OUTDIR/rucc.txt records the versions and the entry.
set -eu
out=$1
ccver=$2
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
bin=/build/rb/bin
mkdir -p "$out" "$bin"
start=$(date +%s)

if ! mount | awk '$3 == "/usr/local"' | grep -q null; then
	mkdir -p /build/local
	cpdup -i0 -q /usr/local /build/local
	mount_null /build/local /usr/local
fi
if ! command -v cargo > /dev/null 2>&1; then
	env ASSUME_ALWAYS_YES=yes pkg install -y rust > "$out/pkg.log" 2>&1 || {
		tail -30 "$out/pkg.log"
		exit 1
	}
fi

export CARGO_HOME=/build/cargo CARGO_TARGET_DIR=/build/rucc-target CARGO_BUILD_JOBS="$RB_JOBS"
(cd /build/rucc-src && cargo build -q --release --offline -p rucc)
install -m 755 "$CARGO_TARGET_DIR/release/rucc" "$bin/rucc"
# compilers.conf names one program for cpp, so a script gives rucc -E.
printf '#!/bin/sh\nexec %s -E "$@"\n' "$bin/rucc" > "$bin/rucc-cpp"
chmod 755 "$bin/rucc-cpp"

# The lines of CCVER are replaced, so a second run gives the same file.
touch /etc/compilers.conf
sed -i '' -e "/^${ccver}_/d" /etc/compilers.conf
cat >> /etc/compilers.conf <<EOF
${ccver}_CC=$bin/rucc
${ccver}_GCC=$bin/rucc
${ccver}_CPP=$bin/rucc-cpp
${ccver}_CXX=/usr/libexec/gcc80/c++
${ccver}_GXX=/usr/libexec/gcc80/g++
${ccver}_INCOPT="\${STD_INCOPT}"
${ccver}_CFLAGS="--sysroot=\${INCPREFIX}"
${ccver}_INCOPTCXX="-isystem /usr/include/c++/8.0"
EOF

{
	echo "# rust"
	rustc --version
	cargo --version
	echo
	echo "# rucc"
	"$bin/rucc" --version 2>&1 | head -3
	echo "$(sha256 -q "$bin/rucc") $bin/rucc"
	echo
	echo "# /etc/compilers.conf"
	cat /etc/compilers.conf
	echo
	echo "# CCVER=$ccver cc --version"
	env CCVER="$ccver" cc --version 2>&1 | head -3
	echo
	echo "# $(($(date +%s) - start))s"
} > "$out/rucc.txt" 2>&1
cat "$out/rucc.txt"
