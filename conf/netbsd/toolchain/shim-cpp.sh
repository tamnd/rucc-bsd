#!/usr/bin/env bash
# The C preprocessor of an R TOOLDIR, installed as bin/x86_64--netbsd-cpp by
# scripts/netbsd/overlay.sh. It runs "rucc -E" with the fixed arguments of shim-cc.sh.
#
# GCC's cpp driver differs from "gcc -E" in two ways that the tree relies on, and this script does
# the same:
#   "cpp IN OUT" writes to OUT, so a second operand becomes "-o OUT";
#   an input with no C or assembler suffix is preprocessed as C (gcc/cppspec.cc), so such an input,
#   or standard input, gets "-x c" when the command line has no -x.
# The log line is that of shim-cc.sh with "cpp" in the third field, and LOG.rusage as there.

rucc=@RUCC@
log=${RB_SHIM_LOG-@LOG@}
timer=@TIME@
use=
if [ -n "$log" ] && [ -n "$timer" ]; then use=$(mktemp "${TMPDIR:-/tmp}/rb-shim.XXXXXXXX"); fi
run() {
	if [ -n "$use" ]; then "$timer" -f '%e %U %S %M' -o "$use" "$@"; else "$@"; fi
}
fixed=(--target=@TARGET@ -fgnuc-version=@GNUC@)
sysroot=--sysroot=@SYSROOT@
opts=()
operands=()
lang=no
skip=no
for a in "$@"; do
	if [ $skip = yes ]; then
		opts+=("$a")
		skip=no
		continue
	fi
	case $a in
	-I | -D | -U | -A | -o | -include | -imacros | -isystem | -idirafter | -iquote | -iprefix | \
		-iwithprefix | -iwithprefixbefore | -isysroot | -MF | -MT | -MQ)
		opts+=("$a")
		skip=yes
		;;
	--sysroot)
		opts+=("$a")
		skip=yes
		sysroot=
		;;
	--sysroot=*)
		opts+=("$a")
		sysroot=
		;;
	-x)
		opts+=("$a")
		skip=yes
		lang=yes
		;;
	-x*)
		opts+=("$a")
		lang=yes
		;;
	-) operands+=("$a") ;;
	-*) opts+=("$a") ;;
	*) operands+=("$a") ;;
	esac
done
in=${operands[0]:--}
out=()
if [ ${#operands[@]} -ge 2 ]; then out=(-o "${operands[1]}"); fi
x=()
if [ $lang = no ]; then
	case $in in
	*.c | *.h | *.i | *.S | *.s | *.sx) ;;
	*) x=(-x c) ;;
	esac
fi
run "$rucc" -E "${fixed[@]}" ${sysroot:+"$sysroot"} "${opts[@]}" "${x[@]}" "$in" "${out[@]}"
status=$?
if [ -n "$log" ]; then
	line="$status	$PWD	cpp"
	for a in "$@"; do line+="	$a"; done
	printf '%s\n' "$line" >> "$log"
fi
if [ -n "$use" ]; then
	printf '%s\t%s\n' "$(tail -n 1 "$use")" "$line" >> "$log.rusage"
	rm -f "$use"
fi
exit $status
