#!/usr/bin/env bash
# The FreeBSD builder VM on a Linux host with KVM (plan 14.6).
#
#   builder.sh up       download and check the image, make the disks, boot, wait for ssh
#   builder.sh ssh CMD  run CMD as root in the VM
#   builder.sh put SRC DST, builder.sh get SRC DST   copy a file in or out
#   builder.sh down     power off and wait for QEMU to exit
#
# The VM gets RB_VM_CPUS vCPUs and RB_VM_MEM of memory. Its root disk is the BASIC-CI image. A second
# disk of RB_BUILD_DISK, sparse, holds the source, the object tree and the image (/build in the VM).
# State lives in RB_VM_DIR. Downloads are kept in RB_CACHE.
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=../../conf/freebsd/builder.conf
. "$here/../../conf/freebsd/builder.conf"

: "${RB_CACHE:=$HOME/.cache/rb}"
: "${RB_VM_DIR:=/mnt/rb-builder}"
: "${RB_VM_CPUS:=$(nproc)}"
: "${RB_VM_MEM:=12G}"
: "${RB_BUILD_DISK:=100G}"
: "${RB_SSH_PORT:=2222}"

key="$RB_VM_DIR/id_ed25519"
ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
	-o ServerAliveInterval=30 -o ServerAliveCountMax=20 -p "$RB_SSH_PORT")

vm_ssh() {
	ssh "${ssh_opts[@]}" -i "$key" -o BatchMode=yes root@127.0.0.1 "$@"
}

up() {
	mkdir -p "$RB_CACHE" "$RB_VM_DIR"
	local xz="$RB_CACHE/$RB_BUILDER_NAME"
	if [ ! -f "$xz" ] || ! echo "$RB_BUILDER_SHA256  $xz" | sha256sum -c --status -; then
		curl -sfL --retry 3 -o "$xz.part" "$RB_BUILDER_URL"
		mv "$xz.part" "$xz"
	fi
	echo "$RB_BUILDER_SHA256  $xz" | sha256sum -c -
	xz -dc -T0 "$xz" > "$RB_VM_DIR/root.raw"
	# growfs in the image grows the root file system to this size at first boot.
	truncate -s 24G "$RB_VM_DIR/root.raw"
	truncate -s "$RB_BUILD_DISK" "$RB_VM_DIR/build.raw"
	rm -f "$key" "$key.pub"
	ssh-keygen -q -t ed25519 -N '' -f "$key"

	qemu-system-x86_64 -machine q35,accel=kvm -cpu host -smp "$RB_VM_CPUS" -m "$RB_VM_MEM" \
		-drive if=virtio,file="$RB_VM_DIR/root.raw",format=raw,cache=unsafe,discard=unmap \
		-drive if=virtio,file="$RB_VM_DIR/build.raw",format=raw,cache=unsafe,discard=unmap \
		-netdev user,id=n0,hostfwd=tcp:127.0.0.1:"$RB_SSH_PORT"-:22 -device virtio-net-pci,netdev=n0 \
		-display none -serial file:"$RB_VM_DIR/console.log" \
		-pidfile "$RB_VM_DIR/qemu.pid" -daemonize

	# The image lets root in with an empty password. The first login installs a key, and every
	# later call uses the key.
	local i
	for i in $(seq 1 60); do
		echo "ssh try $i"
		if sshpass -p '' ssh "${ssh_opts[@]}" -o PubkeyAuthentication=no -o ConnectTimeout=5 \
			root@127.0.0.1 'mkdir -p -m 700 .ssh && cat >> .ssh/authorized_keys' < "$key.pub" 2>/dev/null; then
			vm_ssh 'uname -a; sysctl -n hw.ncpu hw.physmem'
			return 0
		fi
		sleep 10
	done
	echo "the builder VM did not answer on ssh in 10 minutes" >&2
	tail -50 "$RB_VM_DIR/console.log" >&2 || true
	return 1
}

down() {
	[ -f "$RB_VM_DIR/qemu.pid" ] || return 0
	local pid
	pid="$(cat "$RB_VM_DIR/qemu.pid")"
	vm_ssh 'shutdown -p now' >/dev/null 2>&1 || true
	for _ in $(seq 1 60); do
		kill -0 "$pid" 2>/dev/null || return 0
		sleep 2
	done
	kill "$pid" 2>/dev/null || true
}

case "${1:-}" in
	up) up ;;
	down) down ;;
	ssh) shift; vm_ssh "$@" ;;
	put) vm_ssh "cat > '$3'" < "$2" ;;
	get) vm_ssh "cat '$2'" > "$3" ;;
	*) echo "usage: $0 up | down | ssh CMD | put SRC DST | get SRC DST" >&2; exit 2 ;;
esac
