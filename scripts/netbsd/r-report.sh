#!/usr/bin/env bash
# Say how far an R build got, as markdown, from the shim log and the build log.
#
#   r-report.sh BUILD.log [KEEP-GOING.log]
#
# The shim log ($NB_LOGS/shim.tsv) has one line per compiler call. The report gives the number of
# calls and failures, the first failed call with the source file it compiled, and that call's
# error, which it gets by running the call again alone so that the output of parallel jobs does
# not mix with it. KEEP-GOING.log is the log of a "make -k" run after the failure; its failures are
# in the shim log too, so the counts cover every unit make could reach.

. "$(dirname "$0")/common.sh"

build_log=$1
shim="$NB_LOGS/shim.tsv"
touch "$shim"

calls=$(wc -l < "$shim")
failed=$(awk -F '\t' '$1 != 0' "$shim" | wc -l)
units=$(awk -F '\t' '$1 != 0 { for (i = 4; i <= NF; i++) if ($i ~ /\.(c|S|s)$/) print $i }' "$shim" \
	| sort -u | wc -l)

echo "#### Compiler calls through the shims"
echo
echo "| Compiler calls | Failed calls | Failed source files |"
echo "|---|---|---|"
echo "| $calls | $failed | $units |"
echo

first=$(awk -F '\t' '$1 != 0 { print; exit }' "$shim")
if [ -n "$first" ]; then
	IFS=$'\t' read -r -a f <<< "$first"
	dir=${f[1]}
	slot=${f[2]}
	args=("${f[@]:3}")
	file=
	for a in "${args[@]}"; do
		case $a in *.c | *.S | *.s) file=$a ;; esac
	done
	echo "**First failed call:** \`$slot\` on \`${file:-no source file}\` in \`${dir#"$NB_OBJ/"}\`."
	echo
	echo '```'
	(cd "$dir" && RB_SHIM_LOG="" "${NB_TOOLDIR:-$NB_ROOT/tools-r}/bin/x86_64--netbsd-$([ "$slot" = cc ] && echo gcc || echo cpp)" \
		"${args[@]}" 2>&1 >/dev/null | head -40)
	echo '```'
	echo
	echo "The ten source files that failed first:"
	echo
	awk -F '\t' '$1 != 0 { for (i = 4; i <= NF; i++) if ($i ~ /\.(c|S|s)$/) print $i }' "$shim" \
		| awk '!seen[$0]++' | head -10 | sed 's/^/- `/; s/$/`/'
	echo
fi

if [ -f "$build_log" ]; then
	echo "**The first error lines of the build log:**"
	echo
	echo '```'
	grep -n -m 20 -E 'error:|\*\*\* |Error code|failed|not found' "$build_log" | cut -c1-300
	echo '```'
fi
if [ -n "${2:-}" ] && [ -f "$2" ]; then
	echo
	echo "**Messages counted in the make -k log** (the most common first):"
	echo
	echo '```'
	grep -E 'error:' "$2" | sed -E 's/^[^ ]*: ?error: //' | sort | uniq -c | sort -rn | head -15 \
		| cut -c1-200
	echo '```'
fi
