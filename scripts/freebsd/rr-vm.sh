#!/usr/bin/env bash
# A test image, such as the RR image, booted as a host that the job drives by ssh (plan 16.11, B10).
#
#   rr-vm.sh up IMAGE   boot IMAGE by BIOS on a qcow2 overlay, wait for ssh
#   rr-vm.sh ssh CMD    run CMD as root in the VM
#   rr-vm.sh put SRC DST, rr-vm.sh get SRC DST   copy a file in or out
#   rr-vm.sh down       let rb-rc finish and power off, and unpack its results to RB_VM_DIR/results
#
# The image is never written. It needs rb-rc and nothing else: the rb disk names the bench suite
# and holds guest/hold.sh as bench.sh, with the public key of the job as authorized_keys. hold.sh
# starts sshd and waits, so the image boots as it does for boot.sh and then takes commands, as the
# builder VM does (builder.sh). The disks are the overlay (vtbd0), the rb disk (vtbd1, where rb-rc
# looks for it), and a sparse disk of RB_BUILD_DISK for /build (vtbd2). The network is QEMU's user
# network with a route out, so that pkg can fetch, and port RB_SSH_PORT of the host goes to port 22.
# The VM gets RB_VM_CPUS vCPUs and RB_VM_MEM of memory.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
: "${RB_VM_DIR:=/mnt/rb-rr}"
: "${RB_VM_CPUS:=$(nproc)}"
: "${RB_VM_MEM:=12G}"
: "${RB_BUILD_DISK:=100G}"
: "${RB_SSH_PORT:=2223}"
: "${RB_BOOT_LIMIT:=900}"

key="$RB_VM_DIR/id_ed25519"
ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
	-o ServerAliveInterval=30 -o ServerAliveCountMax=20 -p "$RB_SSH_PORT")

vm_ssh() {
	ssh "${ssh_opts[@]}" -i "$key" -o BatchMode=yes root@127.0.0.1 "$@"
}

up() {
	local image=$1 i
	mkdir -p "$RB_VM_DIR"
	rm -f "$key" "$key.pub" "$RB_VM_DIR/console.log"
	ssh-keygen -q -t ed25519 -N '' -f "$key"
	qemu-img create -q -f qcow2 -F raw -b "$(realpath "$image")" "$RB_VM_DIR/disk.qcow2"
	rm -rf "$RB_VM_DIR/rb-in"
	mkdir -p "$RB_VM_DIR/rb-in"
	cat > "$RB_VM_DIR/rb-in/rb.conf" <<'EOF'
RB_SUITES="bench"
RB_POWEROFF=yes
EOF
	cp "$here/guest/hold.sh" "$RB_VM_DIR/rb-in/bench.sh"
	cp "$key.pub" "$RB_VM_DIR/rb-in/authorized_keys"
	tar -cf "$RB_VM_DIR/rb.tar" -C "$RB_VM_DIR/rb-in" .
	truncate -s 1G "$RB_VM_DIR/rb.tar"
	rm -f "$RB_VM_DIR/build.raw"
	truncate -s "$RB_BUILD_DISK" "$RB_VM_DIR/build.raw"

	qemu-system-x86_64 -machine q35,accel=kvm -cpu host -smp "$RB_VM_CPUS" -m "$RB_VM_MEM" -no-reboot \
		-drive if=none,id=root,file="$RB_VM_DIR/disk.qcow2",format=qcow2,cache=unsafe \
		-device virtio-blk-pci,drive=root,bootindex=0 \
		-drive if=none,id=rb,file="$RB_VM_DIR/rb.tar",format=raw -device virtio-blk-pci,drive=rb \
		-drive if=none,id=build,file="$RB_VM_DIR/build.raw",format=raw,cache=unsafe,discard=unmap \
		-device virtio-blk-pci,drive=build \
		-netdev user,id=n0,hostfwd=tcp:127.0.0.1:"$RB_SSH_PORT"-:22 -device virtio-net-pci,netdev=n0 \
		-display none -serial file:"$RB_VM_DIR/console.log" \
		-pidfile "$RB_VM_DIR/qemu.pid" -daemonize

	for i in $(seq 1 $((RB_BOOT_LIMIT / 10))); do
		if grep -q '^RB-HOLD' "$RB_VM_DIR/console.log" 2>/dev/null &&
			vm_ssh -o ConnectTimeout=5 true 2>/dev/null; then
			echo "ssh after $((i * 10))s: $(grep -a -m1 '^RB-READY' "$RB_VM_DIR/console.log" | tr -d '\r')"
			vm_ssh 'uname -a; sysctl -n hw.ncpu hw.physmem'
			return 0
		fi
		if grep -q '^panic:' "$RB_VM_DIR/console.log" 2>/dev/null; then
			break
		fi
		sleep 10
	done
	echo "the VM of $image did not answer on ssh in ${RB_BOOT_LIMIT}s" >&2
	tail -50 "$RB_VM_DIR/console.log" >&2 || true
	return 1
}

down() {
	[ -f "$RB_VM_DIR/qemu.pid" ] || return 0
	local pid
	pid="$(cat "$RB_VM_DIR/qemu.pid")"
	vm_ssh 'touch /var/rb/release' > /dev/null 2>&1 || true
	for _ in $(seq 1 90); do
		kill -0 "$pid" 2>/dev/null || break
		sleep 2
	done
	if kill -0 "$pid" 2>/dev/null; then
		echo "no power off in 180s after the release" >&2
		kill "$pid" 2>/dev/null || true
		sleep 2
	fi
	rm -rf "$RB_VM_DIR/results"
	mkdir -p "$RB_VM_DIR/results"
	tar -xf "$RB_VM_DIR/rb.tar" -C "$RB_VM_DIR/results" 2>/dev/null || true
	rm -f "$RB_VM_DIR/disk.qcow2" "$RB_VM_DIR/rb.tar" "$RB_VM_DIR/build.raw"
}

case "${1:-}" in
	up) up "$2" ;;
	down) down ;;
	ssh) shift; vm_ssh "$@" ;;
	put) vm_ssh "cat > '$3'" < "$2" ;;
	get) vm_ssh "cat '$2'" > "$3" ;;
	*) echo "usage: $0 up IMAGE | down | ssh CMD | put SRC DST | get SRC DST" >&2; exit 2 ;;
esac
