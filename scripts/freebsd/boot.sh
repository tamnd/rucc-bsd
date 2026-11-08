#!/usr/bin/env bash
# One boot of a FreeBSD test image under QEMU with KVM (plan 12.3).
#
#   boot.sh IMAGE bios|uefi "SUITES" OUTDIR
#
# The image is never written. The boot runs on a qcow2 overlay of it in OUTDIR. The VM has the
# shape of FreeBSD's CI test VM: 2 vCPUs, 8 GB, the image, the rb disk, five empty 128 MB disks
# for the tests that take disks, and one 128 MB AHCI disk for the cam(4) tests. A seventh virtio
# disk is for the smoke suite. The network is QEMU's user network with no route out.
#
# The rb disk is a tar archive with rb.conf, which names the suites. rb-rc in the image reads it,
# runs the suites, and writes its results back to the same disk. This script then unpacks them to
# OUTDIR/results.
#
# Console markers and limits:
#   RB-READY   rb-rc started; within RB_BOOT_LIMIT seconds (default 600)
#   RB-DONE    the suites finished; within RB_RUN_LIMIT seconds of the start (default 14400)
# On a timeout QEMU is killed. OUTDIR/boot.json says what happened.
set -euo pipefail

image=$1
firmware=$2
suites=$3
out=$4
: "${RB_BOOT_LIMIT:=600}"
: "${RB_RUN_LIMIT:=14400}"
: "${RB_VM_CPUS:=2}"
: "${RB_VM_MEM:=8G}"
: "${RB_KYUA_FILTERS:=}"
: "${OVMF_CODE:=/usr/share/OVMF/OVMF_CODE_4M.fd}"
: "${OVMF_VARS:=/usr/share/OVMF/OVMF_VARS_4M.fd}"

mkdir -p "$out"
out="$(cd "$out" && pwd)"
console="$out/console.log"
rm -f "$console"

qemu-img create -q -f qcow2 -F raw -b "$(realpath "$image")" "$out/disk.qcow2"
mkdir -p "$out/rb-in"
cat > "$out/rb-in/rb.conf" <<EOF
RB_SUITES="$suites"
RB_POWEROFF=yes
RB_SMOKE_DISK=vtbd7
RB_KYUA_FILTERS="$RB_KYUA_FILTERS"
RB_KYUA_LIMIT=$((RB_RUN_LIMIT - RB_BOOT_LIMIT - 300))
EOF
tar -cf "$out/rb.tar" -C "$out/rb-in" .
truncate -s 1G "$out/rb.tar"
for i in 1 2 3 4 5 smoke cam; do
	rm -f "$out/disk-$i"
	truncate -s 128M "$out/disk-$i"
done

fw=()
if [ "$firmware" = uefi ]; then
	cp "$OVMF_VARS" "$out/vars.fd"
	fw=(-drive "if=pflash,format=raw,readonly=on,file=$OVMF_CODE"
		-drive "if=pflash,format=raw,file=$out/vars.fd")
fi
disks=()
for i in 1 2 3 4 5; do
	disks+=(-drive "if=none,id=d$i,file=$out/disk-$i,format=raw" -device "virtio-blk-pci,drive=d$i")
done

start=$(date +%s)
qemu-system-x86_64 -machine q35,accel=kvm -cpu host -smp "$RB_VM_CPUS" -m "$RB_VM_MEM" \
	"${fw[@]}" -no-reboot -display none -serial file:"$console" \
	-drive if=none,id=root,file="$out/disk.qcow2",format=qcow2 -device virtio-blk-pci,drive=root \
	-drive if=none,id=rb,file="$out/rb.tar",format=raw -device virtio-blk-pci,drive=rb \
	"${disks[@]}" \
	-drive if=none,id=smoke,file="$out/disk-smoke",format=raw -device virtio-blk-pci,drive=smoke \
	-device ahci,id=ahci0 -drive if=none,id=cam,file="$out/disk-cam",format=raw \
	-device ide-hd,drive=cam,bus=ahci0.0 \
	-netdev user,id=n0,restrict=on -device virtio-net-pci,netdev=n0 \
	-pidfile "$out/qemu.pid" &
qemu=$!

ready=""
done_line=""
result=timeout
while kill -0 "$qemu" 2>/dev/null; do
	sleep 5
	now=$(($(date +%s) - start))
	if [ -z "$ready" ] && grep -q '^RB-READY' "$console" 2>/dev/null; then
		ready=$now
		echo "RB-READY after ${now}s: $(grep -m1 '^RB-READY' "$console")"
	fi
	if [ -z "$done_line" ] && grep -q '^RB-DONE' "$console" 2>/dev/null; then
		done_line="$(grep -m1 '^RB-DONE' "$console")"
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

if grep -q '^panic:' "$console"; then
	result=panic
elif [ -n "$done_line" ]; then
	result="done"
elif [ -n "$ready" ]; then
	result=no-done
else
	result=no-ready
fi

rm -rf "$out/results"
mkdir -p "$out/results"
tar -xf "$out/rb.tar" -C "$out/results" 2>/dev/null || true
rm -f "$out/disk.qcow2" "$out/rb.tar" "$out"/disk-* "$out/vars.fd"

cat > "$out/boot.json" <<EOF
{"firmware": "$firmware", "suites": "$suites", "result": "$result", "ready_seconds": ${ready:-null}, "seconds": $seconds, "done": "${done_line#RB-DONE }"}
EOF
cat "$out/boot.json"
[ "$result" = "done" ]
