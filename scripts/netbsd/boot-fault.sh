#!/usr/bin/env bash
# Find where a kernel stopped when a boot did not reach rb-rc.
#
#   boot-fault.sh IMAGE KERNEL OUT
#
# The boot under KVM tells only that the guest reset. This boots IMAGE again under TCG with
# QEMU's interrupt log on, so each exception is written with the CPU state. QEMU stops at the
# reset because of -no-reboot. The script keeps the last lines of the log, finds the program
# counters of the last exceptions and names the function of KERNEL that holds each one. Before
# locore turns on paging the kernel runs at its physical address, so an address under
# 0x80000000 is looked up at KERNBASE plus that address too.
#
# OUT gets console.log, int-tail.log and the report, which also goes to standard output as
# markdown. The interrupt log goes through a pipe to tail, so it does not fill the disk.

set -u

img=$1
kernel=$2
out=$3
kernbase=0xffffffff80000000
mkdir -p "$out"
rm -f "$out/disk.qcow2" "$out/int.fifo"
qemu-img create -q -f qcow2 -F raw -b "$(realpath "$img")" "$out/disk.qcow2"
mkfifo "$out/int.fifo"
tail -n 4000 < "$out/int.fifo" > "$out/int-tail.log" &
tailer=$!

start=$(date +%s)
timeout 1200 qemu-system-x86_64 -machine q35,accel=tcg -cpu max -smp 1 -m 8G \
  -display none -monitor none -serial "file:$out/console.log" -no-reboot \
  -d int,cpu_reset,guest_errors -D "$out/int.fifo" \
  -drive "if=none,id=root,file=$out/disk.qcow2,format=qcow2,cache=unsafe" \
  -device virtio-blk-pci,drive=root,bootindex=0 \
  -netdev user,id=n0,restrict=on -device virtio-net-pci,netdev=n0 > "$out/qemu.log" 2>&1
status=$?
wait "$tailer"
rm -f "$out/disk.qcow2" "$out/int.fifo"
took=$(( $(date +%s) - start ))

# The program counter of each exception, oldest first, without repeats next to each other.
grep -ao 'pc=[0-9a-f]\+' "$out/int-tail.log" | sed 's/pc=//' | uniq | tail -n 12 > "$out/pcs"

syms=$(mktemp)
nm -n "$kernel" 2>/dev/null | awk '$2 ~ /^[tTwW]$/ { print $1, $3 }' > "$syms"

name() {
  local pc=$1 a best
  a=$(( 0x$pc )) 2>/dev/null || { echo "?"; return; }
  if [ "$a" -ge 0 ] && [ "$a" -lt $((0x80000000)) ]; then
    a=$(( a + kernbase ))
  fi
  a=$(printf '%016x' "$a")
  # The last function that starts at or below the address. nm writes 16 hex digits, so the
  # addresses compare as strings.
  best=$(awk -v a="$a" '$1 <= a { line = $0 } $1 > a { exit } END { print line }' "$syms")
  if [ -z "$best" ]; then
    echo "?"
    return
  fi
  printf '%s+0x%x\n' "${best#* }" $(( 0x$a - 0x${best%% *} ))
}

{
  echo "### Where the kernel stopped"
  echo
  echo "QEMU under TCG ran for $took seconds and exited with $status."
  echo
  echo "The last program counters at an exception, oldest first:"
  echo
  echo '| pc | function |'
  echo '|---|---|'
  while read -r pc; do
    echo "| \`$pc\` | \`$(name "$pc")\` |"
  done < "$out/pcs"
  echo
  echo "The last lines of the interrupt log:"
  echo
  echo '```'
  tail -n 60 "$out/int-tail.log"
  echo '```'
  echo
  echo "The last lines of the console:"
  echo
  echo '```'
  tr -d '\r' < "$out/console.log" | tr -s '|/\\-' | tail -n 20
  echo '```'
} | tee "$out/report.md"
rm -f "$syms" "$out/pcs"
