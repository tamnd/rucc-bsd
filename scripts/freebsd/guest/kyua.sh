#!/bin/sh
# The system's own tests (plan 12.2): kyua over /usr/tests. The same script runs on R and G.
#
#   kyua.sh OUTDIR
#
# RB_KYUA_FILTERS, if set, names the test directories or programs to run, for a shard. RB_KYUA_LIMIT
# is the whole-run limit in seconds. When it runs out, kyua is stopped, and the cases it did not
# finish are missing from the results, which the host counts as timeouts.
#
# Before the run, the tests in /usr/local/etc/rb/kyua-exclude are taken out of the Kyuafiles in
# /usr/tests. This changes the installed test list, not the source tree.
out=$1
mkdir -p "$out"
: "${RB_KYUA_LIMIT:=14400}"
db="$out/kyua.db"

exclude()
{
	local entry dir prog parent base
	sed -e 's/#.*//' /usr/local/etc/rb/kyua-exclude | while read -r entry; do
		[ -n "$entry" ] || continue
		case $entry in
		*:*)
			dir=${entry%%:*}
			prog=${entry#*:}
			[ -f "/usr/tests/$dir/Kyuafile" ] &&
			sed -i '' -e "/name=\"$prog\"/s/^/-- /" "/usr/tests/$dir/Kyuafile"
			;;
		*)
			parent=$(dirname "$entry")
			base=$(basename "$entry")
			[ -f "/usr/tests/$parent/Kyuafile" ] &&
			sed -i '' -e "/include(\"$base\/Kyuafile\")/s/^/-- /" "/usr/tests/$parent/Kyuafile"
			;;
		esac
		echo "$entry" >> "$out/excluded"
	done
}

exclude
cd /usr/tests || exit 1
start=$(date +%s)
# RB_KYUA_FILTERS is a list of words, so it is not quoted.
# shellcheck disable=SC2086
timeout -s TERM -k 60 "$RB_KYUA_LIMIT" kyua test -k /usr/tests/Kyuafile --results-file="$db" \
	${RB_KYUA_FILTERS} > "$out/test.log" 2>&1
echo $? > "$out/exit"
echo $(($(date +%s) - start)) > "$out/seconds"

kyua report --results-file="$db" --results-filter passed,skipped,xfail,broken,failed \
	--output "$out/report.txt"
kyua report --results-file="$db" --verbose --results-filter passed,skipped,xfail,broken,failed \
	--output "$out/report-verbose.txt"
kyua report-junit --results-file="$db" --output "$out/junit.xml"
sed -n 's/^Test cases: //p' "$out/report.txt" | tr ' ' '_' > "$out/counts"
