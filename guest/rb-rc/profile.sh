#!/bin/sh
# The profile suite of B8 (plan 13.3): a kernel profile of some benchmarks of the bench suite.
#
#   profile.sh OUT
#
# The bench-profile workflow puts this script on the rb disk as bench.sh, so the bench suite of
# the image runs it instead of guest/rb-rc/bench.sh. It reads the same settings:
#   RB_BENCH        the rb-bench program to run
#   rb_bench_only   the benchmarks to profile, separated by spaces: names of rb-bench -l and
#                   seq_read, the read of a 1 GiB file as in bench.sh
#   rb_bench_scale  multiply the work by this number, 1 by default
#
# Each benchmark runs once under DTrace. The probe profile-997 samples each CPU 997 times per
# second. OUT/NAME.prof gets one line for each sample class and each function:
#   N CLASS COUNT        CLASS is kernel, idle or user
#   F COUNT MODULE`FUNC  the kernel samples outside the idle thread, by function
#   S COUNT              the 40 hottest kernel stacks follow, one frame on each line
# OUT/bench.tsv gets the line of the benchmark, as in bench.sh. OUT/kernel.gz is /boot/kernel/kernel
# of the image, so that the host can disassemble the hot functions. OUT/hwpmc.txt says if hwpmc
# works in the VM.

out=$1
mkdir -p "$out"
work=${RB_WORK:-/var/rb}
scale=${rb_bench_scale:-1}
bench=${RB_BENCH:-/usr/libexec/rb/rb-bench}
tsv=$out/bench.tsv
log=$out/bench.log
tmp=$work/profile
: > "$tsv"
: > "$log"
rm -rf "$tmp"
mkdir -p "$tmp"

gzip -c /boot/kernel/kernel > "$out/kernel.gz"
sysctl kern.compiler_version kern.hz kern.timecounter.hardware hw.model > "$out/sysctl.txt" 2>&1
"$bench" -V > "$out/version" 2>> "$log"

# A benchmark may take 600 seconds, with timeout(1) where the system has it.
if command -v timeout > /dev/null 2>&1; then
	limited="timeout -k 10 600"
else
	limited=
fi

kldload dtraceall >> "$log" 2>&1
if kldload hwpmc >> "$out/hwpmc.txt" 2>&1 || kldstat -q -m hwpmc; then
	pmccontrol -L >> "$out/hwpmc.txt" 2>&1
	$limited pmcstat -S unhalted_core_cycles -O "$tmp/pmc.out" sleep 1 >> "$out/hwpmc.txt" 2>&1
	echo "pmcstat: exit $?" >> "$out/hwpmc.txt"
fi

# The idle threads are not counted as kernel work. execname is a built-in variable, so the
# script does not need the CTF of the kernel.
script='
BEGIN { printf("RB-PROFILE-READY\n"); }
profile-997 /arg0 != 0 && execname != "idle"/ { @f[func(arg0)] = count(); @s[stack(12)] = count(); @n["kernel"] = count(); }
profile-997 /arg0 != 0 && execname == "idle"/ { @n["idle"] = count(); }
profile-997 /arg1 != 0/ { @n["user"] = count(); }
END {
	trunc(@s, 40);
	printa("N %s %@u\n", @n);
	printa("F %@u %a\n", @f);
	printa("S %@u%k\n", @s);
}'

total=0
done_=0

# profiled NAME CMD [ARG ...]: run CMD under DTrace and keep its line of the results.
profiled()
{
	name=$1
	shift
	total=$((total + 1))
	echo "RB-CASE bench/$name"
	echo "== $name" >> "$log"
	dtrace -q -x aggsortrev -x bufsize=16m -x aggsize=16m -n "$script" -o "$tmp/$name.prof" 2>> "$log" &
	dpid=$!
	n=0
	until grep -q RB-PROFILE-READY "$tmp/$name.prof" 2> /dev/null; do
		n=$((n + 1))
		if [ $n -gt 60 ] || ! kill -0 $dpid 2> /dev/null; then
			echo "== $name: dtrace did not start" >> "$log"
			kill $dpid 2> /dev/null
			return
		fi
		sleep 1
	done
	# shellcheck disable=SC2086
	if line=$($limited "$@" 2>> "$log") && [ -n "$line" ]; then
		printf '%s\n' "$line" >> "$tsv"
		done_=$((done_ + 1))
	fi
	# A job in the background of sh ignores SIGINT, so dtrace gets SIGTERM, which also prints
	# the aggregations.
	kill -TERM $dpid
	wait $dpid
	grep -v RB-PROFILE-READY "$tmp/$name.prof" > "$out/$name.prof"
	$RB_SAVE
}

for name in ${rb_bench_only:-}; do
	case $name in
	seq_read)
		mib=$(awk -v s="$scale" 'BEGIN { v = int(1024 * s); if (v < 1) v = 1; print v }')
		dd if=/dev/zero of="$tmp/seq" bs=1048576 count="$mib" 2> /dev/null && sync
		profiled seq_read "$bench" -x fs seq_read dd if="$tmp/seq" of=/dev/null bs=1048576
		rm -f "$tmp/seq"
		;;
	*)
		profiled "$name" "$bench" -s "$scale" "$name"
		;;
	esac
done

rm -rf "$tmp"
echo "$done_/$total" > "$out/counts"
