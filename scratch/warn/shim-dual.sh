#!/usr/bin/env bash
# Scratch: the C compiler slot runs the reference GCC as it is, so the build is the G build. After a
# compile of a .c file that the reference passed, it also runs rucc on the same command line with
# the output and dependency files sent elsewhere, and keeps both standard errors.

ref="$0.ref"
rucc=@RUCC@
dir=@DIR@
gerr=$(mktemp)
"$ref" "$@" 2> "$gerr"
status=$?
cat "$gerr" >&2

compile= src= skip=
for a in "$@"; do
	case $a in
	-c) compile=1 ;;
	-E | -M | -MM | - | -S) skip=1 ;;
	*.c) src=$a ;;
	esac
done
if [ -z "$compile" ] || [ -n "$skip" ] || [ -z "$src" ] || [ $status -ne 0 ]; then
	rm -f "$gerr"
	exit $status
fi

out=$(mktemp)
rargs=() sysroot=--sysroot=@SYSROOT@ drop= drop_o= has_o=
for a in "$@"; do
	if [ -n "$drop" ]; then
		drop=
		if [ "$drop_o" = 1 ]; then rargs+=("$out"); fi
		drop_o=
		continue
	fi
	case $a in
	-o) rargs+=(-o); drop=1 drop_o=1 has_o=1 ;;
	-o?*) rargs+=("-o$out"); has_o=1 ;;
	-MF | -MT | -MQ) drop=1 ;;
	-MD | -MMD | -MP | -Wp,-M*) ;;
	--sysroot | --sysroot=*) sysroot= ; rargs+=("$a") ;;
	*) rargs+=("$a") ;;
	esac
done
if [ -z "$has_o" ]; then rargs+=(-o "$out"); fi
rerr=$(mktemp)
timeout 600 "$rucc" --target=x86_64--netbsd -fgnuc-version=@GNUC@ ${sysroot:+"$sysroot"} "${rargs[@]}" \
	> /dev/null 2> "$rerr" < /dev/null
rstatus=$?
printf '%s\t%s\t%s\t%s\n' "$rstatus" "$([ -s "$gerr" ] && echo w || echo -)" "$PWD" "$src" >> "$dir/units.tsv"
if [ -s "$gerr" ] || [ -s "$rerr" ] || [ $rstatus -ne 0 ]; then
	rec=$(mktemp -d "$dir/u/XXXXXXXX")
	{ printf '%s\n' "$PWD"; printf '%s\n' "$@"; } > "$rec/cmd"
	mv "$gerr" "$rec/gcc"
	mv "$rerr" "$rec/rucc"
	echo "$rstatus" > "$rec/status"
fi
rm -f "$gerr" "$rerr" "$out"
exit $status
