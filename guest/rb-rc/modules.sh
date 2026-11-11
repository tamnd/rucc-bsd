#!/bin/sh
# The modules suite (plan 09.3), installed as /usr/libexec/rb/modules. rb cross-modules uses it.
#
#   modules OUT
#
# NetBSD only. It loads each kernel module, one at a time, with "modload NAME", checks that
# modstat lists it, and unloads it with modunload. The modules are those of the image, under
# /stand/MACHINE/RELEASE/modules. When the results disk has a directory "modules" (rb boot
# --modules DIR), that directory takes the place of the image's first. So a module that a module
# needs comes from the same build.
#
# OUT/modules.tsv gets one line for each module, with a tab between the fields:
#   NAME RESULT ERROR
# RESULT is one of these:
#   pass     it loaded, modstat listed it, and it unloaded
#   present  modstat listed it before the load, because it is in the kernel or an earlier module
#            loaded it; it is not loaded again
#   skip     rb_modules_skip names it
#   load     modload failed
#   list     it loaded, and modstat did not list it
#   unload   modunload failed
# ERROR is the first line of what the failed command printed, or empty.
#
# Before each module, the console gets "RB-CASE modules/NAME", so after a panic the last such
# line names the module. The results are saved after each module. OUT/counts gets
# "passed/total", where present and skipped modules do not count. These settings come from
# rb.conf:
#   rb_modules_only  load only these modules, separated by spaces
#   rb_modules_skip  do not load these modules, such as one that stopped an earlier boot
# The host tests set RB_MODULES to use another directory than the image's.

out=$1
mkdir -p "$out"
base=${RB_MODULES:-/stand/$(uname -m)/$(sysctl -n kern.osrelease)/modules}
disk=${RB_WORK:-/var/rb}/in/modules

if [ -d "$disk" ]; then
	rm -rf "$base.image"
	if [ -d "$base" ]; then
		mv "$base" "$base.image"
	fi
	mkdir -p "$(dirname "$base")"
	mv "$disk" "$base"
	echo disk > "$out/source"
else
	echo image > "$out/source"
fi

# The modules to load: rb_modules_only, else each NAME with a NAME/NAME.kmod.
names()
{
	if [ -n "$rb_modules_only" ]; then
		echo "$rb_modules_only"
		return
	fi
	for d in "$base"/*; do
		n=${d##*/}
		if [ -f "$d/$n.kmod" ]; then
			echo "$n"
		fi
	done
}

listed()
{
	modstat "$1" 2> /dev/null | awk -v m="$1" '$1 == m { f = 1 } END { exit !f }'
}

skipped()
{
	for x in $rb_modules_skip; do
		if [ "$x" = "$1" ]; then
			return 0
		fi
	done
	return 1
}

: > "$out/modules.tsv"
for m in $(names); do
	err=
	if skipped "$m"; then
		result=skip
	else
		echo "RB-CASE modules/$m"
		if listed "$m"; then
			result=present
		elif ! err=$(modload "$m" 2>&1); then
			result=load
		elif ! listed "$m"; then
			result=list
		elif ! err=$(modunload "$m" 2>&1); then
			result=unload
		else
			result=pass
			err=
		fi
	fi
	err=$(echo "$err" | head -n 1 | tr '\t' ' ')
	printf '%s\t%s\t%s\n' "$m" "$result" "$err" >> "$out/modules.tsv"
	if [ -n "$RB_SAVE" ]; then
		$RB_SAVE
	fi
done
awk -F '\t' '$2 != "present" && $2 != "skip" { t++; if ($2 == "pass") p++ }
	END { printf "%d/%d\n", p, t }' "$out/modules.tsv" > "$out/counts"
exit 0
