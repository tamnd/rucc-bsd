#!/usr/bin/env bash
# Look into a kernel link whose nbctfmerge failed, and finish the link without CTF.
#
#   ctfmerge-check.sh CONF BUILD.log
#
# After ld, the kernel Makefile runs nbctfmerge over every object, then moves netbsd to
# netbsd.gdb, copies the debug information to netbsd-CONF.debug and writes netbsd without it. When
# nbctfmerge fails, netbsd is the plain ld output and the steps after it do not run. A "make -k"
# run after that sees netbsd and does nothing.
#
# This script does nothing when netbsd.gdb exists or netbsd does not. Otherwise it:
#
#  1. runs the nbctfmerge command from BUILD.log again under gdb and keeps the backtrace,
#  2. runs nbctfmerge on each object alone and lists the objects that fail alone,
#  3. finds the shortest prefix of the object list that fails, whose last object is the one that
#     makes it fail,
#  4. does the steps after nbctfmerge, so netbsd is the kernel build.sh would make, without CTF.
#
# The report goes to standard output as markdown and to $NB_LOGS/ctfmerge.log.

. "$(dirname "$0")/common.sh"

conf=$1
build_log=$2
tooldir=${NB_TOOLDIR:-$NB_TOOLS}
dir="$NB_OBJ/sys/arch/$NB_MACHINE/compile/$conf"
log="$NB_LOGS/ctfmerge.log"
work=$(mktemp -d)

if [ ! -f "$dir/netbsd" ] || [ -f "$dir/netbsd.gdb" ]; then
	exit 0
fi

# The command as make ran it, from the first line of the log that has it.
line=$(grep -m1 -o 'nbctfmerge -t -g -L VERSION -o netbsd [^&]*' "$build_log" || true)
if [ -z "$line" ]; then
	echo "netbsd exists and netbsd.gdb does not, but $build_log has no nbctfmerge command."
	exit 0
fi
read -r -a words <<< "${line#nbctfmerge }"
# words: -t -g -L VERSION -o netbsd OBJECT...
flags=("${words[@]:0:4}")
objects=("${words[@]:6}")
ctfmerge="$tooldir/bin/nbctfmerge"

# Merge OBJECTS into a copy of vers.o and give the exit status.
merge() {
	cp "$dir/vers.o" "$work/out.o"
	(cd "$dir" && "$ctfmerge" "${flags[@]}" -o "$work/out.o" "$@") > /dev/null 2>&1
}

{
	echo "#### nbctfmerge"
	echo
	echo "ld linked netbsd, and nbctfmerge over its ${#objects[@]} objects failed."
	echo

	command -v gdb > /dev/null || sudo apt-get install -y -qq gdb > /dev/null 2>&1 || true
	cp "$dir/netbsd" "$work/netbsd"
	if command -v gdb > /dev/null; then
		echo "The same command under gdb, with the output in a copy of netbsd:"
		echo
		echo '```'
		(cd "$dir" && gdb -q -batch -ex run -ex bt -ex 'info registers rip' \
			--args "$ctfmerge" "${flags[@]}" -o "$work/netbsd" "${objects[@]}" 2>&1) \
			| grep -v '^\[' | tail -40
		echo '```'
		echo
	fi

	alone=()
	for o in "${objects[@]}"; do
		merge "$o" || alone+=("$o $?")
	done
	echo "Objects that fail alone: ${#alone[@]}."
	if [ "${#alone[@]}" -gt 0 ]; then
		echo
		echo '```'
		printf '%s\n' "${alone[@]}" | head -40
		echo '```'
	fi
	echo

	# The shortest failing prefix, if the whole list fails into the small output as well.
	n=${#objects[@]}
	if merge "${objects[@]}"; then
		echo "All ${n} objects merge into a copy of vers.o, so the failure needs netbsd as the output."
	else
		lo=1
		hi=$n
		while [ "$lo" -lt "$hi" ]; do
			mid=$(((lo + hi) / 2))
			if merge "${objects[@]:0:mid}"; then lo=$((mid + 1)); else hi=$mid; fi
		done
		echo "The first $lo objects fail and the first $((lo - 1)) do not. Object $lo is \`${objects[lo - 1]}\`."
	fi
	echo

	# The steps of SYSTEM_LD_TAIL_STAGE2 after nbctfmerge.
	objcopy="$tooldir/bin/x86_64--netbsd-objcopy"
	(
		cd "$dir" || exit
		chmod 755 netbsd
		mv -f netbsd netbsd.gdb
		"$objcopy" --only-keep-debug --compress-debug-sections netbsd.gdb "netbsd-$conf.debug"
		"$objcopy" -g -p -R .gnu_debuglink --add-gnu-debuglink="netbsd-$conf.debug" netbsd.gdb netbsd
		chmod 755 netbsd netbsd.gdb "netbsd-$conf.debug"
	)
	echo "The steps after nbctfmerge ran, so netbsd is $(stat -c %s "$dir/netbsd") bytes, without CTF."
} | tee "$log"

rm -rf "$work"
