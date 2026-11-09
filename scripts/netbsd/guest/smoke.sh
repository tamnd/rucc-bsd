#!/bin/sh
# The smoke suite, run in the guest as root by boot.py. The same script runs for R and G.
#
#   smoke.sh OUT
#
# Each check prints one line "RB-CHECK <name> pass" or "RB-CHECK <name> fail" and keeps its output
# in OUT/smoke/<name>.txt. The last line is "RB-SMOKE <passed> <failed>". The exit status is the
# number of failed checks.

out=$1/smoke
mkdir -p "$out"
passed=0
failed=0

check() {
	name=$1
	shift
	if "$@" > "$out/$name.txt" 2>&1; then
		echo "RB-CHECK $name pass"
		passed=$((passed + 1))
	else
		echo "RB-CHECK $name fail"
		failed=$((failed + 1))
	fi
}

hello() {
	cd /tmp || return 1
	printf '#include <stdio.h>\nint main(void) { printf("hello %%d\\n", 6 * 7); return 0; }\n' > rb-hello.c
	cc -O2 -o rb-hello rb-hello.c && ./rb-hello && test "$(./rb-hello)" = "hello 42"
}

loopback() {
	ifconfig lo0 && ping -n -c 3 127.0.0.1 && ping6 -n -c 3 ::1
}

dhcp() {
	ifconfig -a && ifconfig vioif0 | grep -q 'inet '
}

# GENERIC has ext2fs and most file systems built in, and trunk no longer has filemon, so the check loads udf, which GENERIC does not have and the modules set does.
module() {
	modstat && modload udf && modstat udf && modunload udf
}

check uname uname -a
check dmesg sh -c 'dmesg; cp /var/run/dmesg.boot "$0/dmesg.boot"' "$out"
check sysctl sysctl kern.ostype kern.osrelease kern.version hw.machine hw.ncpu hw.physmem64
check shells sh -c 'sh -c "echo sh" && ksh -c "echo ksh" && csh -c "echo csh"'
check tools sh -c 'test "$(echo b a c a | tr " " "\n" | sort -u | wc -l | tr -d " ")" = 3 && test "$(awk "BEGIN { print 6 * 7 }")" = 42 && test "$(echo abc | sed s/b/x/)" = axc && ls -l /bin/ls /usr/bin/awk'
check compiler hello
check loopback loopback
check dhcp dhcp
check mount sh -c 'mount && df -h'
check module module
check tests-present test -f /usr/tests/Atffile
check atf-present sh -c 'command -v atf-run && command -v atf-report'

echo "RB-SMOKE $passed $failed"
echo "$passed $failed" > "$out/counts"
exit "$failed"
