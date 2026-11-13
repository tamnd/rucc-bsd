#!/bin/sh
# The ports row of B10 (plan 16.11), run as root in a booted RR image after pkg add of the rucc
# package, with /build mounted and the ports tree in PORTSDIR.
#
#   ports.sh OUTDIR LIST
#
# LIST names one port origin per line, such as math/gmp, with # comments. For each port, the ports
# framework builds it with CC=cc, which is the rucc of the package, and runs its own test suite
# with make test. Before the build, pkg install takes each dependency that the port names from the
# package repository of the image, so rucc builds the port and not what it depends on. A dependency
# with no package is built from the ports tree, with the same CC. CC_REF, such as gcc15, builds the
# ports with that compiler in place of cc, as the control. rucc compiles C only, and the image has
# no C++ compiler, so CXX is CXX_REF, g++15 of the gcc15 package, on both sides. Some configure
# scripts look for a C++ compiler even when they build C only.
#
# OUTDIR/ports.tsv gets one line per port: "origin version build test seconds", where build and
# test are pass or fail, and test is none when the port has no test suite. OUTDIR/ORIGIN.log has
# the output, with / in the origin as -. OUTDIR/counts gets "BUILT/TESTED/TOTAL". OUTDIR/ORIGIN-tests.tar.xz keeps the logs of the test
# suite (test-suite.log, *.log beside *.trs, LastTest.log of CTest, testlog.txt of meson), because
# make clean removes the work directory. It also keeps the *.log files of the dirs that the test
# harness of libarchive leaves in /tmp for each failed test.
#
# The build runs as root, and the test suite runs as the user TEST_USER, as in poudriere, which
# builds and tests as nobody by default. Some suites do not run as root. libuv stops with "The
# libuv test suite cannot be run as root". gawk does not run its PMA test as root. The harness of
# libarchive changes only the effective user ID to nobody when it runs as root, so ld-elf.so.1
# ignores LD_LIBRARY_PATH in the bsdtar, bsdcpio, bsdcat and bsdunzip that the tests start, and
# does not find libarchive.so.13 in the build tree. This happened with rucc and with gcc15 in run
# 43416312862.
set -u
out=$1
list=$2
: "${PORTSDIR:=/build/ports}"
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
: "${CC_REF:=cc}"
: "${CXX_REF:=g++15}"
: "${TEST_USER:=rbtest}"
PATH=/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin
export PATH PORTSDIR
for tool in "$CC_REF" "$CXX_REF"; do
	if ! command -v "$tool" > /dev/null; then
		echo "ports.sh: $tool is not installed" >&2
		exit 2
	fi
done
mkdir -p "$out" /build/ports-wrk /build/distfiles /build/ports-pkg
out=$(cd "$out" && pwd)
: > "$out/ports.tsv"
if ! id "$TEST_USER" > /dev/null 2>&1; then
	pw useradd "$TEST_USER" -m -s /bin/sh -c "ports tests" || exit 2
fi
# Tools that a test suite uses and that no port names as a dependency. test_json_parse_cli of
# devel/json-c makes its input with xxd -r -p, and the image has no xxd. The log goes to tools.log.
env ASSUME_ALWAYS_YES=yes IGNORE_OSVERSION=yes pkg install -y xxd > "$out/tools.log" 2>&1 ||
	echo "ports.sh: pkg install of xxd failed, see tools.log" >&2
# FreeBSD 15 refuses connect(2) to INADDR_ANY by default. connect_unspecified and udp_connect of
# devel/libuv connect to 0.0.0.0 and expect the old behavior, and they failed with ENETUNREACH with
# rucc and with gcc15 in run 43453821594. These sysctls give the old behavior back.
sysctl net.inet.ip.connect_inaddr_wild=1 net.inet6.ip6.connect_in6addr_wild=1 >> "$out/tools.log" 2>&1 ||
	echo "ports.sh: sysctl connect_inaddr_wild failed, see tools.log" >&2
# poll_nested_kqueue of devel/libuv gives a kqueue descriptor to uv_poll_init, which sets it
# nonblocking with ioctl FIONBIO. On FreeBSD 15.1 that ioctl fails with ENOTTY on a kqueue, and
# the test failed in the same way with rucc and with gcc15 in run 43453821594 (test-poll.c:719,
# error -25). The test runner of libuv has no way to skip one case, so this patch takes the case
# out of test/test-list.h.
cat > /build/ports-wrk/libuv-poll-nested-kqueue.patch << 'EOF_PATCH'
--- test/test-list.h.orig
+++ test/test-list.h
@@ -1004,3 +1004,3 @@
 #ifdef UV_HAVE_KQUEUE
-  TEST_ENTRY  (poll_nested_kqueue)
+  /* poll_nested_kqueue is left out by ports.sh of rucc-bsd. */
 #endif
EOF_PATCH
# The cc of FreeBSD 15.1 is clang 19, and its default C is gnu17. gcc15 and rucc default to C23,
# where "void g()" takes no arguments. The long long test in the configure script of math/gmp
# 6.3.0 calls such a g with six, so with C23 the configure script finds no working compiler. This
# make.conf gives that port the C of the stock cc, on both sides.
conf=/build/ports-wrk/make.conf
cat > "$conf" << 'EOF_CONF'
.if ${.CURDIR:M*/math/gmp}
CFLAGS+=	-std=gnu17
.endif
# devel/libuv gets the patch that takes out poll_nested_kqueue. ports.sh writes it.
.if ${.CURDIR:M*/devel/libuv}
EXTRA_PATCHES+=	/build/ports-wrk/libuv-poll-nested-kqueue.patch
.endif
# dns/getaddrinfo_cancel_stress of devel/libevent starts 1000 requests to a DNS server on the
# loopback, and each request has a random cancel timer. It expects that a timer cancels at least
# one request before the answer comes. In this VM all 1000 answers come first. The test failed
# with rucc and with gcc15 in run 43416312862 (regress_dns.c:2105, gaic_freed is 1000), also after
# its 3 tries. A name with ":" in REGRESS_ARGS skips that case in each run of test/regress.
.if ${.CURDIR:M*/devel/libevent}
TEST_ENV=	${MAKE_ENV} REGRESS_ARGS=:dns/getaddrinfo_cancel_stress
.endif
# test-c32tolower.sh and test-c32toupper.sh of the gnulib tests of textproc/gsed fail in the
# zh_CN.GB18030 locale of FreeBSD. The tests expect the other case of a fullwidth letter, such as
# \243\347 for \243\307, and the libc does not give it. They failed in the same way with rucc and
# with gcc15 in run 43416312862 (test-c32tolower.c:402, test-c32toupper.c:420). The other locales
# of these tests pass. XFAIL_TESTS marks only these two tests.
.if ${.CURDIR:M*/textproc/gsed}
TEST_ARGS=	${MAKE_ARGS} XFAIL_TESTS="test-c32tolower.sh test-c32toupper.sh"
.endif
# The port removes -Wno-psabi from the test flags of devel/libffi, for clang. gcc then writes a
# -Wpsabi note for the complex float and AVX vector arguments, and dejagnu counts the note as an
# excess error in cls_align_complex_float.c and vector_double4.c. That failed with gcc15 only in
# run 43416312862. This gives the flag back to gcc only. rucc does not write these notes.
.if ${.CURDIR:M*/devel/libffi} && ${CC:M*gcc*}
TEST_ARGS=	${MAKE_ARGS} RUNTESTFLAGS=--target_board=unix/-Wno-psabi
.endif
EOF_CONF
# The flags of a batch build. CPP is the preprocessor of the same compiler.
set -- __MAKE_CONF="$conf" BATCH=yes DISABLE_VULNERABILITIES=yes NO_IGNORE=yes \
	WRKDIRPREFIX=/build/ports-wrk DISTDIR=/build/distfiles PACKAGES=/build/ports-pkg \
	MAKE_JOBS_NUMBER="$RB_JOBS" CC="$CC_REF" CPP="$CC_REF -E" \
	CXX="$CXX_REF" CXXCPP="$CXX_REF -E" PORTSDIR="$PORTSDIR"
# A for loop and not a while read loop, so that no make or pkg in the loop reads the list. An
# origin has no space in it.
# shellcheck disable=SC2013
for origin in $(sed -e 's/#.*//' "$list"); do
	log=$out/$(echo "$origin" | tr / -).log
	dir=$PORTSDIR/$origin
	start=$(date +%s)
	version=$(make -C "$dir" "$@" -V PKGVERSION 2>/dev/null || echo unknown)
	build=fail
	test=none
	{
		echo "# $origin $version, CC=$CC_REF, CXX=$CXX_REF"
		"$CC_REF" --version 2>&1 | head -1
		echo
		echo "# pkg install of the dependencies"
	} > "$log"
	deps=$(make -C "$dir" "$@" build-depends-list run-depends-list test-depends-list 2>> "$log" |
		sed -e "s|^$PORTSDIR/||" -e 's/@.*//' | sort -u)
	for d in $deps; do
		env ASSUME_ALWAYS_YES=yes IGNORE_OSVERSION=yes pkg install -y "$d" >> "$log" 2>&1 ||
			echo "no package of $d, so the ports tree builds it" >> "$log"
	done
	wrk=$(make -C "$dir" "$@" -V WRKDIR)
	tmpmark=/build/ports-wrk/tmp-mark
	touch "$tmpmark"
	echo "# make build" >> "$log"
	if make -C "$dir" "$@" build >> "$log" 2>&1; then
		build=pass
		if [ -n "$(make -C "$dir" "$@" -V TEST_TARGET)" ] || make -C "$dir" "$@" -V USES | grep -q 'testing'; then
			test=fail
			# make test does the stage first. The stage runs as root, then the test as TEST_USER.
			echo "# make stage" >> "$log"
			if make -C "$dir" "$@" stage >> "$log" 2>&1 && chown -R "$TEST_USER" "$wrk"; then
				echo "# make test, as $TEST_USER" >> "$log"
				# shellcheck disable=SC2016
				if su "$TEST_USER" -c 'exec timeout 3600 "$0" "$@"' make -C "$dir" "$@" test >> "$log" 2>&1; then
					test=pass
				fi
			fi
		fi
	fi
	printf '%s\t%s\t%s\t%s\t%s\n' "$origin" "$version" "$build" "$test" "$(($(date +%s) - start))" >> "$out/ports.tsv"
	echo "$origin $version: build $build, test $test"
	if [ -d "$wrk" ]; then
		# A .trs or a .sum has its .log beside it.
		(cd "$wrk" && find . \( -name test-suite.log -o -name '*.trs' -o -name '*.sum' -o -name LastTest.log \
			-o -name testlog.txt -o -name 'tests.log' \) -print | sed -e 's/\.trs$/.log/' -e 'p' -e 's/\.sum$/.log/' |
			sort -u | while read -r f; do
			[ -f "$f" ] && echo "$f"
		done | tar -cJf "$out/$(echo "$origin" | tr / -)-tests.tar.xz" -T - 2>/dev/null) || true
	fi
	# The dirs of the failed tests of libarchive, such as /tmp/bsdtar_test.DATE.
	find /tmp -maxdepth 1 -type d -name '*_test.*' -newer "$tmpmark" > /build/ports-wrk/tmp-dirs
	if [ -s /build/ports-wrk/tmp-dirs ]; then
		# shellcheck disable=SC2046
		find $(cat /build/ports-wrk/tmp-dirs) -type f -name '*.log' -size -1024k |
			tar -cJf "$out/$(echo "$origin" | tr / -)-tmp.tar.xz" -T - 2>/dev/null || true
		xargs rm -rf < /build/ports-wrk/tmp-dirs
	fi
	make -C "$dir" "$@" clean > /dev/null 2>&1 || true
done
total=$(wc -l < "$out/ports.tsv" | tr -d ' ')
built=$(awk -F'\t' '$3 == "pass"' "$out/ports.tsv" | wc -l | tr -d ' ')
tested=$(awk -F'\t' '$4 == "pass"' "$out/ports.tsv" | wc -l | tr -d ' ')
echo "$built/$tested/$total" > "$out/counts"
echo "built $built, tests passed $tested, of $total ports"
