#!/bin/sh
# Build rucc natively in the DragonFly builder VM and install it as a CCVER compiler through
# /etc/compilers.conf (plan 05.3 and 16.12). Run as root in the VM after provision.sh.
#
#   rucc.sh OUTDIR CCVER
#
# With RB_RUCC_BIN set, rucc.sh installs that rucc and does not build one.
# The source of rucc is in /build/rucc-src, copied in by the host. cargo fetches the crates of
# Cargo.lock from crates.io through the network of the VM, and then builds offline. Rust is the
# rust package of the DragonFly package repository, as rustup has no host toolchain for
# DragonFly. /usr/local of the live system is on its root disk, which has little free space, so
# the packages go to /build/local, which is mounted on /usr/local with mount_null.
#
# compilers.conf gets CCVER_CC, CCVER_GCC and CCVER_CPP for rucc. In the world stage of buildworld,
# the cc script of /usr/libexec/custom sets INCPREFIX to the world tree. rucc gets STD_INCOPT of
# /etc/defaults/compilers.conf, the include options of the base compilers, and --sysroot of the
# world tree, so it links with the crt files and libraries of the world tree, as the gcc80 of the
# cross-tools does. These options are in CCVER_CFLAGS and CCVER_CPPFLAGS, because the cc script
# also gives CCVER_INCOPT to C++. C++ stays with the reference, as in the R builds of FreeBSD and
# NetBSD: the c++ of gcc80 in the cross-tools of the build, with no options, which is what the G
# build runs. The GCC is that of RB_CCVER_REF, gcc80 by default. Outside a build it is the c++ of
# the base system. OUTDIR/rucc.txt records the versions and the entry.
set -eu
out=$1
ccver=$2
ref=${RB_CCVER_REF:-gcc80}
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
bin=/build/rb/bin
mkdir -p "$out" "$bin"
start=$(date +%s)

if [ -n "${RB_RUCC_BIN:-}" ]; then
	# The self-build (RR) gets the rucc of the R build, so it needs no Rust.
	install -m 755 "$RB_RUCC_BIN" "$bin/rucc"
else
	if ! mount | awk '$3 == "/usr/local"' | grep -q null; then
		mkdir -p /build/local
		cpdup -i0 -q /usr/local /build/local
		mount_null /build/local /usr/local
	fi
	# The live system has openssl 3.0 and curl in /usr/local. The rust package needs the
	# libssl.so.17 of security/openssl and the libcurl of the repository. Two packages of the
	# repository have the name openssl (security/openssl and security/openssl30), so pkg does not
	# upgrade openssl for the dependency of rust. openssl is installed by its origin, and curl is
	# upgraded with its dependencies, before rust. Packages of the live system that still need the
	# libssl.so.12 of openssl 3.0, such as libssh2, are then installed again from the repository.
	if ! command -v cargo > /dev/null 2>&1; then
		{
			env ASSUME_ALWAYS_YES=yes pkg install -y security/openssl &&
			env ASSUME_ALWAYS_YES=yes pkg upgrade -y ftp/curl &&
			env ASSUME_ALWAYS_YES=yes pkg install -y lang/rust &&
			old=$( (pkg shlib -qR libssl.so.12 2>/dev/null || true) | sed 's/-[^-]*$//' | tr '\n' ' ') &&
			echo "packages that need libssl.so.12: $old" &&
			if [ -n "$old" ]; then
				# shellcheck disable=SC2086
				env ASSUME_ALWAYS_YES=yes pkg install -f -y $old
			fi
		} > "$out/pkg.log" 2>&1 || {
			tail -30 "$out/pkg.log"
			exit 1
		}
	fi
	pkg query '%n %v %o' openssl curl rust >> "$out/pkg.log" 2>&1 || true
	if ldd "$(command -v cargo)" "$(command -v rustc)" | grep 'not found'; then
		ldd "$(command -v cargo)"
		cat "$out/pkg.log"
		exit 1
	fi
	export CARGO_HOME=/build/cargo CARGO_TARGET_DIR=/build/rucc-target CARGO_BUILD_JOBS="$RB_JOBS"
	(cd /build/rucc-src && cargo fetch --locked && cargo build -q --release --locked --offline -p rucc)
	install -m 755 "$CARGO_TARGET_DIR/release/rucc" "$bin/rucc"
fi
# compilers.conf names one program for cpp. rpcgen runs it on .x files (include/rpc), so it must
# act as cpp and not as a compiler driver.
cat > "$bin/rucc-cpp" <<'CPPEOF'
#!/bin/sh
# cpp of the rucc CCVER, as the rb-cpp of crates/rb-shim does it (decide_cpp in route.rs).
# cpp [options] [infile [outfile]] becomes rucc -E. The second operand becomes -o outfile.
# GCC's cpp reads an input of any suffix as C, and so does standard input, but a compiler
# driver gives a .x file to the linker, so -x c comes first when no -x is given. With no input
# file, cpp reads standard input.
n=$#
take=
lang=
operands=0
inputs=0
other=
while [ "$n" -gt 0 ]; do
	a=$1
	shift
	n=$((n - 1))
	if [ -n "$take" ]; then
		take=
		set -- "$@" "$a"
		continue
	fi
	case $a in
	-o|-I|-D|-U|-A|-include|-imacros|-isystem|-iquote|-idirafter|-iprefix|-iwithprefix|-iwithprefixbefore|-isysroot|--sysroot|-MF|-MT|-MQ)
		take=yes
		set -- "$@" "$a"
		;;
	-x)
		take=yes
		lang=yes
		set -- "$@" "$a"
		;;
	-x*)
		lang=yes
		set -- "$@" "$a"
		;;
	-*)
		if [ "$a" != - ]; then
			set -- "$@" "$a"
			continue
		fi
		operands=$((operands + 1))
		if [ "$operands" -ne 2 ]; then
			inputs=$((inputs + 1))
			other=yes
			set -- "$@" "$a"
		fi
		;;
	*)
		operands=$((operands + 1))
		if [ "$operands" -eq 2 ]; then
			set -- "$@" -o "$a"
			continue
		fi
		inputs=$((inputs + 1))
		case $a in
		*.c|*.h|*.i|*.S|*.s|*.sx) ;;
		*) other=yes ;;
		esac
		set -- "$@" "$a"
		;;
	esac
done
if [ "$inputs" -eq 0 ]; then
	set -- "$@" -
	other=yes
fi
if [ -z "$lang" ] && [ -n "$other" ]; then
	set -- -x c "$@"
fi
exec "$(dirname "$0")/rucc" -E "$@"
CPPEOF
chmod 755 "$bin/rucc-cpp"

# The lines of CCVER are replaced, so a second run gives the same file.
touch /etc/compilers.conf
sed -i '' -e "/^${ccver}_/d" /etc/compilers.conf
cat >> /etc/compilers.conf <<EOF
${ccver}_GCCDIR=/usr/libexec/$ref
if [ -x "\${INCPREFIX}/../ctools_x86_64_x86_64/usr/libexec/$ref/c++" ]; then
	${ccver}_GCCDIR="\${INCPREFIX}/../ctools_x86_64_x86_64/usr/libexec/$ref"
fi
${ccver}_CC=$bin/rucc
${ccver}_GCC=$bin/rucc
${ccver}_CPP=$bin/rucc-cpp
${ccver}_CXX=\${${ccver}_GCCDIR}/c++
${ccver}_GXX=\${${ccver}_GCCDIR}/g++
${ccver}_INCOPT=
${ccver}_INCOPTCXX=
${ccver}_CFLAGS="--sysroot=\${INCPREFIX} \${STD_INCOPT}"
${ccver}_CPPFLAGS="--sysroot=\${INCPREFIX} \${STD_INCOPT}"
EOF

{
	echo "# rust"
	if [ -n "${RB_RUCC_BIN:-}" ]; then
		echo "none, rucc is $RB_RUCC_BIN"
	else
		rustc --version
		cargo --version
	fi
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
