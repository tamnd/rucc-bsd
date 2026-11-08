#!/bin/sh
# Run part of /usr/tests with atf-run, as anita does, in the guest as root.
#
#   atf.sh OUT all
#   atf.sh OUT only DIR...
#   atf.sh OUT except DIR...
#
# The top-level directories come from the "tp:" lines of /usr/tests/Atffile. Each directory runs as
# its own atf-run from inside it, and its raw output goes to OUT/atf/DIR.tps, which is what the host
# parses. atf-report's text and XML outputs are kept next to it. A line "RB-ATF <dir> <status>
# <seconds>" goes to the console after each directory, so the console log shows progress.
#
# The transfer disk is rewritten after each directory, so a run that is killed later still leaves
# the directories that finished.

root=$1
out=$root/atf
mode=$2
shift 2
mkdir -p "$out"

all=$(sed -n 's/^tp:[[:space:]]*//p' /usr/tests/Atffile)
dirs=
for d in $all; do
	case $mode in
	all) dirs="$dirs $d" ;;
	only) for x in "$@"; do [ "$x" = "$d" ] && dirs="$dirs $d"; done ;;
	except)
		keep=yes
		for x in "$@"; do [ "$x" = "$d" ] && keep=no; done
		[ $keep = yes ] && dirs="$dirs $d"
		;;
	esac
done
echo "RB-ATF-DIRS$dirs"
echo "$all" > "$out/all-dirs"
echo "$dirs" > "$out/dirs"

for d in $dirs; do
	start=$(date +%s)
	(cd "/usr/tests/$d" && atf-run > "$out/$d.tps" 2> "$out/$d.err")
	status=$?
	atf-report -o "ticker:$out/$d.txt" -o "xml:$out/$d.xml" < "$out/$d.tps"
	end=$(date +%s)
	echo "$d $status $((end - start))" >> "$out/status"
	echo "RB-ATF $d $status $((end - start))"
	(cd "$root" && tar cf /dev/rld1d .)
done
exit 0
