#!/bin/sh
# The system's own test suite (plan 12.2), installed as /usr/libexec/rb/tests.
#
#   tests OUT
#
# FreeBSD runs kyua over /usr/tests. NetBSD runs atf-run in each top-level directory of
# /usr/tests, as anita does. The raw outputs go to OUT. The host parses them (rb-atf):
#   FreeBSD  OUT/kyua.log (the lines of kyua test), OUT/kyua.db, OUT/report.txt and
#            OUT/report-junit.xml
#   NetBSD   OUT/atf/DIR.tps (the output of atf-run), OUT/atf/DIR.xml and OUT/atf/status
#
# On NetBSD a directory can be under the top, such as net/carp. Its output is then
# OUT/atf/net/carp.tps. A directory that is named again runs again, and its output of run N is
# OUT/atf/DIR@N.tps, so rb_tests_only="net/carp net/carp" runs net/carp twice. A test program,
# such as lib/libc/sys/t_aio_lio, can be named in place of a directory, and only it runs.
#
# The console gets one "RB-CASE <dir>/<program>:<case>" line before each test case on NetBSD,
# and the lines of kyua test on FreeBSD. After a panic, the last of them names the test that was
# running.
#
# OUT/counts gets one line "passed/total". These settings come from rb.conf:
#   rb_tests_only        run only these directories or test programs, separated by spaces
#   rb_tests_except      run every top-level directory but these, or with rb_tests_only, every
#                        directory of rb_tests_only but these. An entry can be under a
#                        directory that runs, such as net/ipsec. That directory then runs as
#                        the entries of its Atffile or Kyuafile, without net/ipsec.
#   rb_kyua_parallelism  kyua's parallelism, default 2 (plan 12.2)
# The host tests set RB_TESTS to use another directory than /usr/tests.

out=$1
mkdir -p "$out"
tests=${RB_TESTS:-/usr/tests}

# The top-level directories. The top Kyuafile of FreeBSD has no include lines. It is Lua that
# includes each directory with a Kyuafile, so top does the same.
top()
{
	if [ -f $tests/Kyuafile ]; then
		for d in "$tests"/*/Kyuafile; do
			if [ -f "$d" ]; then
				d=${d%/Kyuafile}
				echo "${d##*/}"
			fi
		done
	else
		sed -n 's/^tp:[[:space:]]*//p' $tests/Atffile
	fi
}

# The entries of directory $1: the test programs and directories of its Kyuafile, or the entries
# of its Atffile. Nothing when it has neither.
children()
{
	if [ -f "$tests/$1/Kyuafile" ]; then
		sed -n -e 's/^[a-z]*_test_program{name="\([^"]*\)".*/\1/p' \
			-e 's/^include("\([^"]*\)\/Kyuafile")$/\1/p' "$tests/$1/Kyuafile"
	elif [ -f "$tests/$1/Atffile" ]; then
		sed -n 's/^tp:[[:space:]]*//p' "$tests/$1/Atffile"
	fi
}

# Print $1, a directory or a test program, or nothing when rb_tests_except has it. When an entry
# of rb_tests_except is under $1, print each entry of $1 (see children) in place of $1, with the
# same rule. The body is a subshell, so each call has its own variables, and exit ends only that
# call.
entries()
(
	for x in $rb_tests_except; do
		if [ "$x" = "$1" ]; then
			exit 0
		fi
	done
	for x in $rb_tests_except; do
		case $x in
		"$1"/*)
			c=$(children "$1")
			if [ -n "$c" ]; then
				for e in $c; do
					entries "$1/$e"
				done
				exit 0
			fi
			;;
		esac
	done
	echo "$1"
)

# The directories to run. Empty means all, in one run.
pick()
{
	if [ -n "$rb_tests_only" ]; then
		for d in $rb_tests_only; do
			entries "$d"
		done
		return
	fi
	if [ -z "$rb_tests_except" ]; then
		return
	fi
	for d in $(top); do
		entries "$d"
	done
}

dirs=$(pick)
top > "$out/all-dirs"
echo "$dirs" > "$out/dirs"

if [ -f $tests/Kyuafile ]; then
	cd $tests || exit 1
	# shellcheck disable=SC2086
	kyua -v parallelism="${rb_kyua_parallelism:-2}" test --results-file="$out/kyua.db" $dirs 2>&1 |
		tee "$out/kyua.log"
	kyua report --results-file="$out/kyua.db" --verbose > "$out/report.txt" 2>&1
	kyua report-junit --results-file="$out/kyua.db" --output="$out/report-junit.xml" 2>&1
	awk '/  ->  / { t++ } /  ->  passed/ { p++ } END { printf "%d/%d\n", p, t }' \
		"$out/kyua.log" > "$out/counts"
elif [ -f $tests/Atffile ]; then
	mkdir -p "$out/atf"
	: > "$out/atf/status"
	# NetBSD riscv64 loads a module more than 2 GiB from the kernel, out of reach of a call, and
	# the first call of the module panics the kernel (tamnd/rucc-bsd#179). Some tests autoload a
	# module, such as compat_13 or fss. On riscv64 the tests run with autoload off, so that these
	# tests fail and do not panic the kernel. The console line puts this in the report.
	case $(uname -m) in
	riscv*)
		if sysctl -w kern.module.autoload=0 > /dev/null 2>&1; then
			echo "RB-NOTE tests kern.module.autoload=0: NetBSD riscv64 loads modules out of reach of a call (tamnd/rucc-bsd#179)"
		else
			echo "RB-NOTE tests kern.module.autoload stays on: sysctl -w failed"
		fi
		;;
	esac
	for d in ${dirs:-$(top)}; do
		start=$(date +%s)
		f=$d
		n=1
		while [ -e "$out/atf/$f.tps" ]; do
			n=$((n + 1))
			f=$d@$n
		done
		mkdir -p "$(dirname "$out/atf/$f")"
		# A test program, such as lib/libc/sys/t_aio_lio, runs alone from its directory.
		dir=$d
		prog=
		if [ ! -d "$tests/$d" ]; then
			dir=$(dirname "$d")
			prog=$(basename "$d")
		fi
		# shellcheck disable=SC2086
		(cd "$tests/$dir" && atf-run $prog 2> "$out/atf/$f.err") |
			tee "$out/atf/$f.tps" |
			awk -F', ' -v d="$dir" '
				/^tp-start: / { p = $2 }
				/^tc-start: / { print "RB-CASE " d "/" p ":" $2; fflush() }'
		atf-report -o "xml:$out/atf/$f.xml" < "$out/atf/$f.tps" > /dev/null 2>&1
		echo "$f $(($(date +%s) - start))" >> "$out/atf/status"
		echo "RB-TESTS-DIR $d"
		if [ -n "$RB_SAVE" ]; then
			$RB_SAVE
		fi
	done
	find "$out/atf" -name '*.tps' -exec cat {} + 2> /dev/null |
		awk -F', ' '/^tc-end: / { t++; if ($3 == "passed") p++ } END { printf "%d/%d\n", p, t }' \
		> "$out/counts"
else
	echo "no $tests/Kyuafile or $tests/Atffile" > "$out/status"
	exit 1
fi
exit 0
