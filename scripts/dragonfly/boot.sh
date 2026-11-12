#!/usr/bin/env bash
# One boot of a DragonFly test image under QEMU with KVM (plan 12.3), as scripts/freebsd/boot.sh.
#
#   boot.sh IMAGE "SUITES" OUTDIR
#
# The image is never written. The boot runs on a qcow2 overlay of it in OUTDIR. The VM has 2 vCPUs
# and 4 GB. The image, the rb disk and an empty 128 MB disk for the smoke suite are on the AHCI
# controller of q35, so they are da0, da1 and da2. The image boots with BIOS only, as the image
# has no EFI slice. The network card is an e1000, em0, on QEMU's user network with no route out.
#
# The rb disk is a tar archive with rb.conf, which names the suites, and the suite scripts. The rb
# rc script of the image reads it, runs the suites, and writes its results back to the same disk.
# This script then unpacks them to OUTDIR/results.
#
# Console markers and limits:
#   RB-READY   rb started; within RB_BOOT_LIMIT seconds (default 600)
#   RB-DONE    the suites finished; within RB_RUN_LIMIT seconds of the start (default 3600)
# On a timeout QEMU is killed. OUTDIR/boot.json says what happened.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
image=$1
suites=$2
out=$3
: "${RB_BOOT_LIMIT:=600}"
: "${RB_RUN_LIMIT:=3600}"
: "${RB_VM_CPUS:=2}"
: "${RB_VM_MEM:=4G}"

mkdir -p "$out"
out="$(cd "$out" && pwd)"
console="$out/console.log"
rm -f "$console"

qemu-img create -q -f qcow2 -F raw -b "$(realpath "$image")" "$out/disk.qcow2"
rm -rf "$out/rb-in"
mkdir -p "$out/rb-in"
cat > "$out/rb-in/rb.conf" <<CONF
RB_SUITES="$suites"
RB_POWEROFF=yes
RB_SMOKE_DISK=da2
CONF
cp "$here/guest/smoke.sh" "$out/rb-in/"
tar -cf "$out/rb.tar" -C "$out/rb-in" .
truncate -s 256M "$out/rb.tar"
rm -f "$out/disk-smoke"
truncate -s 128M "$out/disk-smoke"

start=$(date +%s)
# graphics=off makes SeaBIOS copy the screen to the serial port, so a boot that stops in boot2
# shows why. The loader and the kernel use the serial console of loader.conf.
qemu-system-x86_64 -machine q35,accel=kvm,graphics=off -cpu "${RB_VM_CPU:-Nehalem}" -smp "$RB_VM_CPUS" -m "$RB_VM_MEM" \
	-no-reboot -display none -monitor none -serial file:"$console" \
	-drive if=none,id=root,file="$out/disk.qcow2",format=qcow2 -device ide-hd,drive=root,bus=ide.0 \
	-drive if=none,id=rb,file="$out/rb.tar",format=raw -device ide-hd,drive=rb,bus=ide.1 \
	-drive if=none,id=smoke,file="$out/disk-smoke",format=raw -device ide-hd,drive=smoke,bus=ide.2 \
	-netdev user,id=n0,restrict=on -device e1000,netdev=n0 \
	-pidfile "$out/qemu.pid" &
qemu=$!

ready=""
done_line=""
while kill -0 "$qemu" 2>/dev/null; do
	sleep 5
	now=$(($(date +%s) - start))
	if [ -z "$ready" ] && grep -aq '^RB-READY' "$console" 2>/dev/null; then
		ready=$now
		echo "RB-READY after ${now}s: $(grep -a -m1 '^RB-READY' "$console" | tr -d '\r')"
	fi
	if [ -z "$done_line" ] && grep -aq '^RB-DONE' "$console" 2>/dev/null; then
		done_line="$(grep -a -m1 '^RB-DONE' "$console" | tr -d '"\\\r')"
		echo "$done_line after ${now}s"
	fi
	if [ -z "$ready" ] && [ "$now" -gt "$RB_BOOT_LIMIT" ]; then
		echo "no RB-READY in ${RB_BOOT_LIMIT}s"
		kill "$qemu"
		break
	fi
	if [ "$now" -gt "$RB_RUN_LIMIT" ]; then
		echo "no RB-DONE in ${RB_RUN_LIMIT}s"
		kill "$qemu"
		break
	fi
done
wait "$qemu" || true
seconds=$(($(date +%s) - start))

if grep -aq '^panic:' "$console"; then
	result=panic
elif [ -n "$done_line" ]; then
	result="done"
elif [ -n "$ready" ]; then
	result=no-done
else
	result=no-ready
fi

if [ "$result" = panic ]; then
	grep -a -m1 -B2 -A30 '^panic:' "$console" > "$out/panic.txt" || true
	cat "$out/panic.txt"
fi

rm -rf "$out/results"
mkdir -p "$out/results"
tar -xf "$out/rb.tar" -C "$out/results" 2>/dev/null || true
rm -f "$out/disk.qcow2" "$out/rb.tar" "$out/disk-smoke"

cat > "$out/boot.json" <<EOF
{"suites": "$suites", "result": "$result", "ready_seconds": ${ready:-null}, "seconds": $seconds, "done": "${done_line#RB-DONE }", "panic": "$(grep -a -m1 '^panic:' "$console" | tr -d '"\\\r' || true)"}
EOF
cat "$out/boot.json"
[ "$result" = "done" ]
