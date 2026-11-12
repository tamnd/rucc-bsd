#!/bin/sh
# The hold suite of a test image (plan 16.11, B10). scripts/freebsd/rr-vm.sh puts it on the rb disk
# as bench.sh, and rb-rc runs it as the bench suite, so any image with rb-rc can run it.
#
#   hold.sh OUTDIR
#
# It lets root in by ssh with the key authorized_keys in RB_WORK, starts sshd, says RB-HOLD on the
# console, and waits until the host makes the file RB_WORK/release. Then rb-rc writes its results
# and powers off as after the other suites. While it waits, the host runs its commands by ssh, as on
# the builder VM.
out=$1
: "${RB_WORK:=/var/rb}"
mkdir -p "$out" /root/.ssh
chmod 700 /root/.ssh
cat "$RB_WORK/authorized_keys" >> /root/.ssh/authorized_keys
chmod 600 /root/.ssh/authorized_keys
echo "PermitRootLogin prohibit-password" >> /etc/ssh/sshd_config
service sshd onestart > "$out/sshd.txt" 2>&1
echo "RB-HOLD $(date -u '+%Y-%m-%dT%H:%M:%SZ')" > /dev/console
while [ ! -f "$RB_WORK/release" ]; do
	sleep 5
done
echo "released $(date -u '+%Y-%m-%dT%H:%M:%SZ')" > "$out/counts"
