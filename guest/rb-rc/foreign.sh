#!/bin/sh
# The foreign suite (plan 12.8), installed as /usr/libexec/rb/foreign. rb foreign uses it.
#
#   foreign OUT
#
# It installs packages that the project built, not rucc, and runs a check of each. The packages
# come on the results disk under pkgs (rb boot --pkgs DIR): pkgs/All has the package files, and
# pkgs/checks has one line per package, its name, a tab and a shell command. For each line, the
# package is installed with what it needs from pkgs/All: pkg_add on NetBSD, pkg add on FreeBSD.
# Then sh runs the command in an empty directory, with /usr/pkg and /usr/local first in PATH and
# a limit of rb_foreign_timeout seconds, 300 by default. The check passes when it exits 0. Both
# read /dev/null, so that they do not read the checks.
#
# OUT/foreign.tsv gets one line for each package, with a tab between the fields:
#   NAME RESULT ERROR
# RESULT is pass, add when the install failed, or check when the check failed. ERROR is the last
# line that the failed command printed, or empty. OUT/NAME.log has all of it.
#
# Before each package, the console gets "RB-CASE foreign/NAME", so after a panic the last such
# line names the package. The results are saved after each package. OUT/counts gets
# "passed/total". With no pkgs on the results disk it gets "0/0".

out=$1
mkdir -p "$out"
work=${RB_WORK:-/var/rb}
pkgs=$work/in/pkgs
tmp=$work/foreign
limit=${rb_foreign_timeout:-300}
PATH=/usr/pkg/bin:/usr/pkg/sbin:/usr/local/bin:/usr/local/sbin:$PATH
export PATH

: > "$out/foreign.tsv"
if [ ! -f "$pkgs/checks" ]; then
	echo 0/0 > "$out/counts"
	exit 0
fi

add()
{
	case $(uname -s) in
	FreeBSD)
		ASSUME_ALWAYS_YES=yes
		export ASSUME_ALWAYS_YES
		pkg add "$pkgs/All/$1"-[0-9]*.pkg
		;;
	*)
		PKG_PATH=$pkgs/All
		export PKG_PATH
		pkg_add -v "$1"
		;;
	esac
}

# The last line of the log, with no tab.
last()
{
	tail -n 1 "$1" | tr '\t' ' '
}

tab=$(printf '\t')
while IFS=$tab read -r name check; do
	if [ -z "$name" ]; then
		continue
	fi
	echo "RB-CASE foreign/$name"
	log=$out/$name.log
	err=
	if ! add "$name" < /dev/null > "$log" 2>&1; then
		result=add
		err=$(last "$log")
	else
		rm -rf "${tmp:?}/$name"
		mkdir -p "$tmp/$name"
		echo "+ $check" > "$log.check"
		if (cd "$tmp/$name" && timeout "$limit" sh -c "$check") < /dev/null >> "$log.check" 2>&1; then
			result=pass
		else
			result=check
			err=$(last "$log.check")
		fi
		cat "$log.check" >> "$log"
		rm -f "$log.check"
	fi
	printf '%s\t%s\t%s\n' "$name" "$result" "$err" >> "$out/foreign.tsv"
	if [ -n "$RB_SAVE" ]; then
		$RB_SAVE
	fi
done < "$pkgs/checks"
rm -rf "$tmp"
awk -F '\t' '{ t++; if ($2 == "pass") p++ } END { printf "%d/%d\n", p, t }' \
	"$out/foreign.tsv" > "$out/counts"
exit 0
