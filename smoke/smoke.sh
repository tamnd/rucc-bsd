#!/bin/sh
# The smoke suite of plan 12.2 and 02.3 part 3. The same script runs for R and G, on FreeBSD and
# on NetBSD. rb image installs it as /usr/libexec/rb/smoke, and rb-rc runs it as root.
#
#   smoke OUT
#
# The checks:
#   shells          sh and csh run a command
#   network         the virtio interface (vtnet0 or vioif0) has an address from QEMU's DHCP
#                   server, and lo0 answers ping
#   spare           fsck, mount and umount of the partition next to the root, or on NetBSD a
#                   listing of it where the system has mounted it already, as riscv mounts its
#                   EFI system partition on /boot
#   package         install and delete one package from the local repository in the image
#   module          load and unload one kernel module
#   tests-present   /usr/tests has its Kyuafile or Atffile, and kyua or atf-run is there
#
# Each check keeps its output in OUT/<name>.log and prints "RB-CHECK <result> <name>" on the
# console. The result is pass, fail or skip. A check is skipped only when the image has nothing
# for it, for example no package repository. OUT/results has one "<result> <name>" line per
# check. OUT/counts has one line "passed/total", and skipped checks are not in the total.
#
# These settings come from rb.conf:
#   rb_pkg_dir         the local package repository, default /var/db/rb-pkg
#   rb_smoke_package   the package to install, default the first one in the repository
#   rb_smoke_module    the module, default nullfs on FreeBSD and udf on NetBSD, which GENERIC
#                      does not have built in
#   rb_smoke_spare     the spare partition, default /dev/gpt/rbspare on FreeBSD and the EFI
#                      system partition on NetBSD

out=$1
mkdir -p "$out"
: > "$out/results"
sys=$(uname -s)
pkg_dir=${rb_pkg_dir:-/var/db/rb-pkg}
mnt=/tmp/rb-spare

# A check function returns 0 to pass, 1 to fail and 2 to skip.
check()
{
	name=$1
	shift
	"$@" > "$out/$name.log" 2>&1
	case $? in
	0) result=pass ;;
	2) result=skip ;;
	*) result=fail ;;
	esac
	echo "$result $name" >> "$out/results"
	echo "RB-CHECK $result $name"
}

shells()
{
	test "$(sh -c 'echo sh')" = sh || return 1
	test "$(csh -fc 'echo csh')" = csh || return 1
}

network()
{
	case $sys in
	FreeBSD) ifn=vtnet0 ;;
	NetBSD) ifn=vioif0 ;;
	*) return 1 ;;
	esac
	ifconfig lo0 || return 1
	ping -c 1 127.0.0.1 || return 1
	# DHCP may still be running when rb-rc starts.
	n=0
	while [ $n -lt 30 ]; do
		if ifconfig $ifn | grep 'inet 10\.0\.2\.'; then
			return 0
		fi
		sleep 1
		n=$((n + 1))
	done
	ifconfig -a
	return 1
}

# The NetBSD image has no spare partition of its own, so the check uses the EFI system partition,
# a msdos wedge next to the root. dkctl names it. The root's disk comes first: on evbarm and riscv
# it is ld4, because ld0 to ld3 are wired to sdmmc.
netbsd_spare()
{
	root=$(sysctl -n kern.root_device 2> /dev/null)
	root=$(dkctl "$root" getwedgeinfo 2> /dev/null | sed -n 's/^dk[0-9]* at \([a-z]*[0-9]*\): .*/\1/p')
	for d in $root ld0 wd0 sd0; do
		w=$(dkctl $d listwedges 2> /dev/null | sed -n 's/^\(dk[0-9]*\): .*type: msdos.*/\1/p')
		if [ -n "$w" ]; then
			echo "$w" | head -n 1
			return
		fi
	done
}

spare()
{
	mkdir -p $mnt
	case $sys in
	FreeBSD)
		dev=${rb_smoke_spare:-/dev/gpt/rbspare}
		test -e "$dev" || return 2
		fsck_ffs -y "$dev" || return 1
		mount -o ro "$dev" $mnt || return 1
		;;
	NetBSD)
		w=${rb_smoke_spare:-$(netbsd_spare)}
		test -n "$w" || return 2
		# The riscv image mounts its EFI system partition on /boot, and a second mount of it
		# fails. The check then reads the mount that is there, with no fsck and no mount.
		at=$(mount | sed -n "s|^/dev/$w on \([^ ]*\) .*|\1|p" | head -n 1)
		if [ -n "$at" ]; then
			echo "/dev/$w is mounted on $at"
			ls -l "$at" || return 1
			test "$(ls "$at" | wc -l)" -gt 0
			return
		fi
		fsck_msdos -n "/dev/r$w" || return 1
		mount_msdos -o ro "/dev/$w" $mnt || return 1
		;;
	*)
		return 1
		;;
	esac
	ls -l $mnt
	ok=$(ls $mnt | wc -l)
	umount $mnt || return 1
	test "$ok" -gt 0
}

package()
{
	test -d "$pkg_dir" || return 2
	case $sys in
	FreeBSD)
		all=$pkg_dir/All
		test -d "$all" || all=$pkg_dir
		name=${rb_smoke_package:-$(ls "$all" | sed -n '/^pkg-[0-9]/d; s/-[0-9][^-]*\.pkg$//p' | head -n 1)}
		test -n "$name" || return 2
		ASSUME_ALWAYS_YES=yes
		export ASSUME_ALWAYS_YES
		if ! pkg -N > /dev/null 2>&1; then
			pkg add "$all"/pkg-[0-9]*.pkg || return 1
		fi
		pkg add "$all/$name"-[0-9]*.pkg || return 1
		pkg info -e "$name" || return 1
		pkg delete -y "$name" || return 1
		;;
	NetBSD)
		name=${rb_smoke_package:-$(ls "$pkg_dir" | sed -n 's/-[0-9][^-]*\.tgz$//p' | head -n 1)}
		test -n "$name" || return 2
		PKG_PATH=$pkg_dir pkg_add -v "$name" || return 1
		pkg_info -e "$name" || return 1
		pkg_delete "$name" || return 1
		;;
	*)
		return 1
		;;
	esac
}

module()
{
	case $sys in
	FreeBSD)
		m=${rb_smoke_module:-nullfs}
		if ! kldstat -q -m "$m"; then
			kldload "$m" || return 1
		fi
		kldstat -v -n "$m" || return 1
		kldunload "$m" || return 1
		! kldstat -q -n "$m"
		;;
	NetBSD)
		m=${rb_smoke_module:-udf}
		modload "$m" || return 1
		modstat "$m" | grep "^$m " || return 1
		modunload "$m" || return 1
		! modstat "$m" | grep -q "^$m "
		;;
	*)
		return 1
		;;
	esac
}

tests_present()
{
	case $sys in
	FreeBSD) test -f /usr/tests/Kyuafile && command -v kyua ;;
	NetBSD) test -f /usr/tests/Atffile && command -v atf-run && command -v atf-report ;;
	*) return 1 ;;
	esac
}

check shells shells
check network network
check spare spare
check package package
check module module
check tests-present tests_present

passed=$(grep -c '^pass ' "$out/results")
failed=$(grep -c '^fail ' "$out/results")
echo "$passed/$((passed + failed))" > "$out/counts"
echo "RB-SMOKE $passed/$((passed + failed))"
exit 0
