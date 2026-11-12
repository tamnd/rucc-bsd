#!/bin/sh
# The bench suite (plan 13.2), installed as /usr/libexec/rb/bench. rb bench uses it.
#
#   bench OUT
#
# It runs the benchmark set once:
#   - rb-bench, the C benchmarks: system calls, processes, memory, events, locking, network
#     and libc. The program is the one on the results disk (rb boot --bench FILE), else
#     /usr/libexec/rb/rb-bench. Each side's image runs the copy its own compiler built.
#   - tools: sh, awk, sed, grep, sort and the compressors over fixed inputs that rb-bench -g
#     writes;
#   - file systems: tar extract, find and rm -r of a tree of small files on a memory disk, and
#     a sequential write and read of 1 GiB in a file under the work directory. The image's root
#     file system has room for 1 GiB, not for the 4 GB of plan 13.2;
#   - crypto: openssl speed and ssh over loopback.
#
# OUT/bench.tsv gets one line for each benchmark, with a tab between the fields:
#   GROUP NAME VALUE BETTER
# VALUE is the wall time in seconds when BETTER is "lower", and a rate when it is "higher". A
# benchmark that cannot run, for example because the system has no zstd, has no line, and
# OUT/bench.log says why. Before each benchmark, the console gets "RB-CASE bench/NAME". OUT/counts
# gets "done/total". These settings come from rb.conf:
#   rb_bench_scale  multiply the work by this number, 1 by default; the host tests use less
#   rb_bench_only   run only these benchmarks, separated by spaces
#   rb_bench_limit  the seconds one benchmark may take, 300 times the scale by default. A
#                   benchmark that takes longer is killed, has no line, and bench.log says so.
# The host tests set RB_BENCH to use another rb-bench.

out=$1
mkdir -p "$out"
work=${RB_WORK:-/var/rb}
scale=${rb_bench_scale:-1}
tsv=$out/bench.tsv
log=$out/bench.log
tmp=$work/bench
: > "$tsv"
: > "$log"
rm -rf "$tmp"
mkdir -p "$tmp"

bench=${RB_BENCH:-}
if [ -z "$bench" ]; then
	if [ -f "$work/in/rb-bench" ]; then
		bench=$tmp/rb-bench
		cp "$work/in/rb-bench" "$bench"
		chmod 555 "$bench"
		echo disk > "$out/source"
	else
		bench=/usr/libexec/rb/rb-bench
		echo image > "$out/source"
	fi
fi
"$bench" -V > "$out/version" 2>> "$log"

total=0
done_=0

# Whether the benchmark NAME is to run.
wanted()
{
	if [ -z "$rb_bench_only" ]; then
		return 0
	fi
	for w in $rb_bench_only; do
		if [ "$w" = "$1" ]; then
			return 0
		fi
	done
	return 1
}

# The limit of one benchmark, with timeout(1) where the system has it.
limit=${rb_bench_limit:-$(awk -v s="$scale" 'BEGIN { v = int(300 * s); if (v < 60) v = 60; print v }')}
if command -v timeout > /dev/null 2>&1; then
	limited="timeout -k 10 $limit"
else
	limited=
fi

# Note a benchmark that ran into the limit: timeout(1) exits with 124 then.
over()
{
	if [ "$1" -eq 124 ] || [ "$1" -eq 137 ]; then
		echo "== $2: killed after the limit of ${limit}s" >> "$log"
		echo "RB-CASE-LIMIT bench/$2 ${limit}s"
	fi
}

# One line of the results. The line is saved at once, so a hang keeps the earlier results.
result()
{
	printf '%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" >> "$tsv"
	done_=$((done_ + 1))
	$RB_SAVE
}

# A fixed amount of work, times the scale, as a whole number of at least 1.
amount()
{
	awk -v n="$1" -v s="$scale" 'BEGIN { v = int(n * s); if (v < 1) v = 1; print v }'
}

# timed GROUP NAME CMD [ARG ...]: the wall time of CMD, through rb-bench -x.
timed()
{
	group=$1
	name=$2
	shift 2
	if ! wanted "$name"; then
		return
	fi
	total=$((total + 1))
	echo "RB-CASE bench/$name"
	echo "== $name" >> "$log"
	# shellcheck disable=SC2086
	if line=$($limited "$bench" -x "$group" "$name" "$@" 2>> "$log"); then
		printf '%s\n' "$line" >> "$tsv"
		done_=$((done_ + 1))
		$RB_SAVE
	else
		over $? "$name"
	fi
}

# Whether the system has the program.
have()
{
	command -v "$1" > /dev/null 2>&1
}

# The C benchmarks, one process each, so a crash costs only one.
for name in $("$bench" -l 2>> "$log" | cut -f 2); do
	if ! wanted "$name"; then
		continue
	fi
	total=$((total + 1))
	echo "RB-CASE bench/$name"
	echo "== $name" >> "$log"
	# shellcheck disable=SC2086
	if line=$($limited "$bench" -s "$scale" "$name" 2>> "$log") && [ -n "$line" ]; then
		printf '%s\n' "$line" >> "$tsv"
		done_=$((done_ + 1))
		$RB_SAVE
	else
		over $? "$name"
	fi
done

# The inputs. They take the place of a fixed corpus: rb-bench -g writes the same bytes on every
# system.
"$bench" -g log "$tmp/log" "$(amount 200)" 2>> "$log"
"$bench" -g lines "$tmp/lines" "$(amount 10000000)" 2>> "$log"
head -c "$(($(amount 64) * 1048576))" "$tmp/log" > "$tmp/corpus" 2>> "$log"

# tools
# sh_loop: parameter expansion, case and arithmetic in sh. Each turn takes 10 characters off s and
# adds at most 9, so s stays under 20 characters. It took 5 and added up to 8, so s grew to
# megabytes, each case scanned all of it, and the loop ran past every limit on FreeBSD and NetBSD.
cat > "$tmp/loop.sh" <<'EOF'
i=0
s=
while [ $i -lt "$1" ]; do
	s="${s#??????????}x$i"
	case $s in
	*7*) n=${#s} ;;
	*) n=$((i % 13)) ;;
	esac
	set -- "$1" "$n" "$s"
	i=$((i + 1))
done
EOF
timed tools sh_loop sh "$tmp/loop.sh" "$(amount 2000000)"
timed tools awk_sum sh -c "awk '{ n[\$3]++; v += substr(\$7, 7) } END { for (k in n) print k, n[k]; print v }' '$tmp/log' > /dev/null"
timed tools sed_subst sh -c "sed -e 's/value=\\([0-9]*\\)/v:\\1/' -e 's/^2026-//' '$tmp/log' > /dev/null"
timed tools grep_fixed sh -c "grep -c -F 'status=error' '$tmp/log' > /dev/null"
timed tools grep_regex sh -c "grep -c -E 'host0[0-9]7 [a-z]+\\[[0-9]*5\\]' '$tmp/log' > /dev/null"
timed tools sort sh -c "sort -o '$tmp/sorted' '$tmp/lines'"
for z in gzip bzip2 xz zstd; do
	if have $z; then
		timed tools "${z}_c" sh -c "$z -c '$tmp/corpus' > '$tmp/corpus.$z'"
		timed tools "${z}_d" sh -c "$z -d -c '$tmp/corpus.$z' > /dev/null"
	else
		echo "== $z: not on this system" >> "$log"
	fi
done
rm -f "$tmp/log" "$tmp/lines" "$tmp/sorted" "$tmp/corpus".*

# file systems: a memory disk with UFS or FFS.
mnt=$tmp/mnt
mkdir -p "$mnt"
size=$(amount 1024)
mounted=
case $(uname -s) in
FreeBSD)
	if mdmfs -s "${size}m" md "$mnt" >> "$log" 2>&1; then
		mounted=yes
	fi
	;;
NetBSD)
	if mount_mfs -s "${size}m" swap "$mnt" >> "$log" 2>&1; then
		mounted=yes
	fi
	;;
esac
if [ -n "$mounted" ]; then
	if "$bench" -g tree "$tmp/tree" "$(amount 70000)" 2>> "$log"; then
		(cd "$tmp" && tar -cf "$tmp/tree.tar" tree) 2>> "$log"
	fi
	rm -rf "$tmp/tree"
	timed fs tar_extract sh -c "cd '$mnt' && tar -xf '$tmp/tree.tar'"
	timed fs find_stat sh -c "find '$mnt/tree' -type f -size +4 > /dev/null"
	timed fs rm_tree rm -rf "$mnt/tree"
	rm -f "$tmp/tree.tar"
	umount "$mnt" >> "$log" 2>&1
else
	echo "== fs: no memory disk" >> "$log"
fi
mib=$(amount 1024)
timed fs seq_write sh -c "dd if=/dev/zero of='$tmp/seq' bs=1048576 count=$mib 2> /dev/null && sync"
timed fs seq_read sh -c "dd if='$tmp/seq' of=/dev/null bs=1048576 2> /dev/null"
rm -f "$tmp/seq"

# crypto: openssl speed gives a rate, from its machine readable lines.
#   +F:N:NAME:R1:...   the bytes per second of each block size; the last is the largest
#   +F2:N:BITS:S:V     the RSA signs and verifies per second
seconds=$(amount 3)
if have openssl; then
	for alg in aes-256-gcm sha256 rsa2048; do
		name=openssl_$(echo "$alg" | tr -d -)
		if ! wanted "$name"; then
			continue
		fi
		total=$((total + 1))
		echo "RB-CASE bench/$name"
		echo "== $name" >> "$log"
		case $alg in
		aes-256-gcm) args="-evp $alg" ;;
		*) args=$alg ;;
		esac
		# shellcheck disable=SC2086
		value=$($limited openssl speed -mr -seconds "$seconds" $args 2>> "$log" |
		    awk -F: '/^\+F:/ { v = $NF } /^\+F2:/ { v = $4 } END { if (v != "") print v }')
		# OpenSSL 3 gives the RSA signs per second. An older one gives the seconds of one sign.
		better=higher
		if [ "$alg" = rsa2048 ] && awk -v v="$value" 'BEGIN { exit !(v < 1) }'; then
			better=lower
		fi
		if [ -n "$value" ]; then
			result crypto "$name" "$value" "$better"
		fi
	done
else
	echo "== openssl: not on this system" >> "$log"
fi

# ssh over loopback, with keys and an sshd of this run only.
if have ssh && [ -x /usr/sbin/sshd ] && wanted ssh_copy; then
	ssh-keygen -q -t ed25519 -N '' -f "$tmp/id" > /dev/null 2>> "$log"
	ssh-keygen -q -t ed25519 -N '' -f "$tmp/host" > /dev/null 2>> "$log"
	/usr/sbin/sshd -f /dev/null -o ListenAddress=127.0.0.1 -o Port=2222 \
	    -o HostKey="$tmp/host" -o AuthorizedKeysFile="$tmp/id.pub" \
	    -o PermitRootLogin=yes -o StrictModes=no -o PidFile="$tmp/sshd.pid" >> "$log" 2>&1
	sleep 1
	timed crypto ssh_copy sh -c "dd if=/dev/zero bs=1048576 count=$(amount 1024) 2> /dev/null | ssh -i '$tmp/id' -p 2222 -o BatchMode=yes -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null \"\$(id -un)\"@127.0.0.1 'cat > /dev/null'"
	if [ -s "$tmp/sshd.pid" ]; then
		kill "$(cat "$tmp/sshd.pid")"
	fi
fi

rm -rf "$tmp"
echo "$done_/$total" > "$out/counts"
