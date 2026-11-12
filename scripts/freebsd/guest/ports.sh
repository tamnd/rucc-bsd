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
# ports with that compiler in place of cc, as the control.
#
# OUTDIR/ports.tsv gets one line per port: "origin version build test seconds", where build and
# test are pass or fail, and test is none when the port has no test suite. OUTDIR/ORIGIN.log has
# the output, with / in the origin as -. OUTDIR/counts gets "BUILT/TESTED/TOTAL".
set -u
out=$1
list=$2
: "${PORTSDIR:=/build/ports}"
: "${RB_JOBS:=$(sysctl -n hw.ncpu)}"
: "${CC_REF:=cc}"
PATH=/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin
export PATH PORTSDIR
mkdir -p "$out" /build/ports-wrk /build/distfiles /build/ports-pkg
out=$(cd "$out" && pwd)
: > "$out/ports.tsv"
# The flags of a batch build. CPP is the preprocessor of the same compiler.
set -- BATCH=yes DISABLE_VULNERABILITIES=yes NO_IGNORE=yes \
	WRKDIRPREFIX=/build/ports-wrk DISTDIR=/build/distfiles PACKAGES=/build/ports-pkg \
	MAKE_JOBS_NUMBER="$RB_JOBS" CC="$CC_REF" CPP="$CC_REF -E" PORTSDIR="$PORTSDIR"
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
		echo "# $origin $version, CC=$CC_REF"
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
	echo "# make build" >> "$log"
	if make -C "$dir" "$@" build >> "$log" 2>&1; then
		build=pass
		if [ -n "$(make -C "$dir" "$@" -V TEST_TARGET)" ] || make -C "$dir" "$@" -V USES | grep -q 'testing'; then
			echo "# make test" >> "$log"
			test=fail
			if timeout 3600 make -C "$dir" "$@" test >> "$log" 2>&1; then
				test=pass
			fi
		fi
	fi
	printf '%s\t%s\t%s\t%s\t%s\n' "$origin" "$version" "$build" "$test" "$(($(date +%s) - start))" >> "$out/ports.tsv"
	echo "$origin $version: build $build, test $test"
	make -C "$dir" "$@" clean > /dev/null 2>&1 || true
done
total=$(wc -l < "$out/ports.tsv" | tr -d ' ')
built=$(awk -F'\t' '$3 == "pass"' "$out/ports.tsv" | wc -l | tr -d ' ')
tested=$(awk -F'\t' '$4 == "pass"' "$out/ports.tsv" | wc -l | tr -d ' ')
echo "$built/$tested/$total" > "$out/counts"
echo "built $built, tests passed $tested, of $total ports"
