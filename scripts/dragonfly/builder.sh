#!/usr/bin/env bash
# The DragonFly builder VM on a Linux host with KVM (plan 14.6).
#
#   builder.sh up       download and check the image, make the disks, boot, log in, start sshd
#   builder.sh ssh CMD  run CMD as root in the VM
#   builder.sh put SRC DST, builder.sh get SRC DST   copy a file in or out
#   builder.sh down     power off and wait for QEMU to exit
#
# The root disk is the official release image of conf/dragonfly/builder.conf, the live system that
# the installer runs from. It is not installed: the build runs in the live system, and its /etc, /var
# and /tmp are memory file systems. A second disk of RB_BUILD_DISK, sparse, holds the source, the
# object tree, the packages and the image (/build in the VM). Both disks are on the AHCI controller
# of q35, so they are da0 and da1. The network card is an e1000, em0.
#
# The image has no serial console and no sshd. console.py stops the loader on the serial port,
# boots with the serial console, logs in as root, and runs the steps that start sshd with a key
# made here. Every later call uses ssh. State lives in RB_VM_DIR. Downloads are kept in RB_CACHE.
#
# With RB_BUILDER_RAW set to a raw test image of image.sh, up boots that image instead (up_image).
set -euo pipefail

here="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=../../conf/dragonfly/builder.conf
. "$here/../../conf/dragonfly/builder.conf"

: "${RB_CACHE:=$HOME/.cache/rb}"
: "${RB_VM_DIR:=/mnt/rb-builder}"
: "${RB_VM_CPUS:=$(nproc)}"
: "${RB_VM_MEM:=12G}"
: "${RB_BUILD_DISK:=100G}"
: "${RB_SSH_PORT:=2222}"
: "${RB_ACCEL:=kvm}"
# The kernel of 6.4.2 stops with a general protection fault on a wrmsr early in the boot when it
# runs with -cpu host on an AMD EPYC runner. A fixed Intel model gives the same CPU on all runners.
: "${RB_VM_CPU:=Nehalem}"

key="$RB_VM_DIR/id_ed25519"
ssh_opts=(-o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -o LogLevel=ERROR
	-o ServerAliveInterval=30 -o ServerAliveCountMax=20 -p "$RB_SSH_PORT")

vm_ssh() {
	ssh "${ssh_opts[@]}" -i "$key" -o BatchMode=yes root@127.0.0.1 "$@"
}

# The self-build (RR) boots a test image of image.sh instead of the release image. The rb rc script
# of the image runs the builder suite of the rb disk, which installs the key and starts sshd. The rb
# disk is da1 and the build disk is da2.
up_image() {
	mkdir -p "$RB_VM_DIR"
	rm -f "$key" "$key.pub" "$RB_VM_DIR/console.log"
	ssh-keygen -q -t ed25519 -N '' -f "$key"
	rm -rf "$RB_VM_DIR/rb-in"
	mkdir -p "$RB_VM_DIR/rb-in"
	printf 'RB_SUITES="builder"\nRB_POWEROFF=no\n' > "$RB_VM_DIR/rb-in/rb.conf"
	cp "$key.pub" "$RB_VM_DIR/rb-in/authorized_keys"
	tar -cf "$RB_VM_DIR/rb.tar" -C "$RB_VM_DIR/rb-in" .
	truncate -s 64M "$RB_VM_DIR/rb.tar"
	truncate -s "$RB_BUILD_DISK" "$RB_VM_DIR/build.raw"
	qemu-system-x86_64 -machine q35,accel="$RB_ACCEL",graphics=off -cpu "$RB_VM_CPU" -smp "$RB_VM_CPUS" -m "$RB_VM_MEM" \
		-drive if=none,id=root,file="$RB_BUILDER_RAW",format=raw,cache=unsafe -device ide-hd,drive=root,bus=ide.0 \
		-drive if=none,id=rb,file="$RB_VM_DIR/rb.tar",format=raw -device ide-hd,drive=rb,bus=ide.1 \
		-drive if=none,id=build,file="$RB_VM_DIR/build.raw",format=raw,cache=unsafe -device ide-hd,drive=build,bus=ide.2 \
		-netdev user,id=n0,hostfwd=tcp:127.0.0.1:"$RB_SSH_PORT"-:22 -device e1000,netdev=n0 \
		-serial file:"$RB_VM_DIR/console.log" -display none -monitor none \
		-pidfile "$RB_VM_DIR/qemu.pid" -daemonize
	for _ in $(seq 1 120); do
		if grep -aq '^RB-BUILDER' "$RB_VM_DIR/console.log" 2>/dev/null &&
			vm_ssh -o ConnectTimeout=5 'uname -a; sysctl -n hw.ncpu hw.physmem' 2>/dev/null; then
			return 0
		fi
		sleep 5
	done
	echo "the image did not answer on ssh" >&2
	tail -c 3000 "$RB_VM_DIR/console.log" >&2 || true
	return 1
}

up() {
	if [ -n "${RB_BUILDER_RAW:-}" ]; then
		up_image
		return
	fi
	mkdir -p "$RB_CACHE" "$RB_VM_DIR"
	local bz2="$RB_CACHE/$RB_BUILDER_NAME"
	local url
	for url in $RB_BUILDER_URLS; do
		if [ -f "$bz2" ] && echo "$RB_BUILDER_SHA256  $bz2" | sha256sum -c --status -; then
			break
		fi
		echo "download $url/$RB_BUILDER_NAME"
		curl -sfL --retry 3 -o "$bz2.part" "$url/$RB_BUILDER_NAME" && mv "$bz2.part" "$bz2" || true
	done
	echo "$RB_BUILDER_SHA256  $bz2" | sha256sum -c -
	echo "$RB_BUILDER_MD5  $bz2" | md5sum -c -
	bzip2 -dc "$bz2" > "$RB_VM_DIR/root.raw"
	truncate -s "$RB_BUILD_DISK" "$RB_VM_DIR/build.raw"
	rm -f "$key" "$key.pub" "$RB_VM_DIR/serial.sock" "$RB_VM_DIR/console.log"
	ssh-keygen -q -t ed25519 -N '' -f "$key"

	local cpu=(-cpu "$RB_VM_CPU")
	[ "$RB_ACCEL" = kvm ] || cpu=()
	# graphics=off makes SeaBIOS copy the screen to the serial port and read the keyboard from it,
	# which console.py needs until the kernel has the serial console.
	qemu-system-x86_64 -machine q35,accel="$RB_ACCEL",graphics=off "${cpu[@]}" -smp "$RB_VM_CPUS" -m "$RB_VM_MEM" \
		-drive if=none,id=root,file="$RB_VM_DIR/root.raw",format=raw,cache=unsafe -device ide-hd,drive=root,bus=ide.0 \
		-drive if=none,id=build,file="$RB_VM_DIR/build.raw",format=raw,cache=unsafe -device ide-hd,drive=build,bus=ide.1 \
		-netdev user,id=n0,hostfwd=tcp:127.0.0.1:"$RB_SSH_PORT"-:22 -device e1000,netdev=n0 \
		-chardev "socket,id=s0,path=$RB_VM_DIR/serial.sock,server=on,wait=off,logfile=$RB_VM_DIR/console.log" \
		-serial chardev:s0 -display none -monitor none \
		-pidfile "$RB_VM_DIR/qemu.pid" -daemonize

	# The live system has root with no password, and an empty /root on a memory file system. Its
	# /etc is a memory file system too, so the change to sshd_config is lost at power off.
	cat > "$RB_VM_DIR/steps" <<STEPS
dhclient em0 > /dev/null
mkdir -p -m 700 /root/.ssh
echo '$(cat "$key.pub")' > /root/.ssh/authorized_keys
sed -i '' -e '/^PermitRootLogin/d' -e '/^UseDNS/d' /etc/ssh/sshd_config
printf 'PermitRootLogin prohibit-password\nUseDNS no\n' >> /etc/ssh/sshd_config
/etc/rc.d/sshd forcestart > /dev/null 2>&1
STEPS
	if ! python3 -I "$here/console.py" "$RB_VM_DIR/serial.sock" "$RB_VM_DIR/steps"; then
		echo "the builder VM did not give a root shell on its console" >&2
		tail -c 3000 "$RB_VM_DIR/console.log" >&2 || true
		return 1
	fi
	for _ in $(seq 1 30); do
		if vm_ssh -o ConnectTimeout=5 'uname -a; sysctl -n hw.ncpu hw.physmem' 2>/dev/null; then
			return 0
		fi
		sleep 5
	done
	echo "the builder VM did not answer on ssh" >&2
	tail -c 3000 "$RB_VM_DIR/console.log" >&2 || true
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
