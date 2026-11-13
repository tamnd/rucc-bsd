#!/bin/sh
# rb-rc (plan 12.4), installed as /usr/libexec/rb/run. /etc/rc.d/rb calls it last at boot.
#
#   run          run the suites, save the results, print RB-DONE and power off
#   run save     write the results so far to the results disk
#
# Steps:
#   1. Print "RB-READY <uname -a>" on the console.
#   2. Read the results disk. If it holds a tar archive with an rb.conf, that file overrides the
#      rb.conf of the image. rb boot uses this to choose the suites at boot time. The archive is
#      unpacked to $work/in, and the rb.conf of rb boot --scripts sets libexec to $work/in/rb,
#      so the suites run with the scripts on the disk. When this script does not know a suite
#      of rb_suites, that rb.conf runs the run script of the disk in its place (rb_rc_suites).
#   3. Save uname and dmesg.
#   4. Run each suite of rb_suites. Print "RB-SUITE <name>" before it and "RB-SUITE-END <name>
#      <counts>" after it.
#   5. Write the results as a tar archive with no file system to the results disk. With no
#      results disk, write the archive to the console with uuencode -m.
#   6. Print "RB-DONE <suite>=<counts> ... archive=<disk|console>" and power off. If the VM is
#      not off after rb_poweroff_wait seconds, print "RB-POWEROFF-FORCE <seconds>", the processes
#      and the mounts, and power off with no sync and no rc.shutdown.
#
# The suites:
#   boot     nothing more to run. The boot passes when RB-DONE comes and the VM powers off.
#   smoke    /usr/libexec/rb/smoke, the smoke suite of plan 12.2.
#   tests    /usr/libexec/rb/tests, kyua on FreeBSD and atf-run on NetBSD.
#   modules  /usr/libexec/rb/modules, which loads and unloads each kernel module on NetBSD.
#   bench    /usr/libexec/rb/bench, the benchmark set of plan 13.2, for rb bench.
#   foreign  /usr/libexec/rb/foreign, which installs the project's own packages and checks each
#            one, for rb foreign.
#
# Nothing is sent on the network, because the network stack is under test. The console carries
# only the markers. The results disk carries the rest.
#
# The host tests run this script with no VM. These variables move its paths:
#   RB_CONF     the image's rb.conf, default /etc/rb.conf
#   RB_WORK     the work directory, default /var/rb
#   RB_LIBEXEC  the suite scripts, default /usr/libexec/rb

PATH=/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin
export PATH

conf=${RB_CONF:-/etc/rb.conf}
work=${RB_WORK:-/var/rb}
libexec=${RB_LIBEXEC:-/usr/libexec/rb}
out=$work/out
# The suites that this script knows. The rb.conf of rb boot --scripts reads it. The rb-rc of an
# older image does not set it, so that rb.conf runs the newer run script of the disk.
rb_rc_suites="boot smoke tests modules bench foreign"

# NetBSD's results disk: the raw partition of the ld disk after the root's disk. The root is
# often a wedge, such as dk1 at ld0. evbarm and riscv wire ld0 to ld3 to sdmmc, so their virtio
# disks start at ld4, and their raw partition is c, not d.
netbsd_disk()
{
	root=$(sysctl -n kern.root_device 2> /dev/null)
	case $root in
	dk[0-9]*) root=$(dkctl "$root" getwedgeinfo 2> /dev/null | sed -n 's/^dk[0-9]* at \([a-z]*[0-9]*\): .*/\1/p') ;;
	esac
	raw=$(sysctl -n kern.rawpartition 2> /dev/null)
	raw=$(echo abcdefghijklmnop | cut -c$((${raw:-3} + 1)))
	case $root in
	ld[0-9]*) echo "/dev/rld$((${root#ld} + 1))$raw" ;;
	*) echo "/dev/rld1$raw" ;;
	esac
}

# The defaults. rb.conf sets the values that differ.
case $(uname -s) in
FreeBSD) rb_disk=/dev/vtbd1 ;;
NetBSD) rb_disk=$(netbsd_disk) ;;
*) rb_disk= ;;
esac
rb_suites=boot
rb_poweroff=YES
rb_poweroff_wait=90
if [ -r "$conf" ]; then
	# shellcheck source=/dev/null
	. "$conf"
fi

# Write the results so far. The archive goes to the start of the raw disk and ends with the two
# zero blocks of tar, so whatever is after it on the disk does not count.
save()
{
	if [ -z "$rb_disk" ] || [ ! -e "$rb_disk" ]; then
		return 1
	fi
	sync
	(cd "$out" && tar -cf "$rb_disk" .)
}

# "run save" comes from a suite script, after the results disk was read. The settings of that
# boot are in $work/conf.
if [ "$1" = save ]; then
	if [ -r "$work/conf" ]; then
		# shellcheck source=/dev/null
		. "$work/conf"
	fi
	save
	exit $?
fi

# The results of one suite: "passed/total" from its counts file, or "error" if it wrote none.
counts()
{
	if [ -s "$1/counts" ]; then
		head -n 1 "$1/counts"
	else
		echo error
	fi
}

rm -rf "$work"
mkdir -p "$work/in" "$out"
echo "RB-READY $(uname -a)"

if [ -n "$rb_disk" ] && [ -e "$rb_disk" ]; then
	if (cd "$work/in" && tar -xf "$rb_disk") 2>/dev/null && [ -f "$work/in/rb.conf" ]; then
		cp "$work/in/rb.conf" "$work/conf"
		# shellcheck source=/dev/null
		. "$work/conf"
		echo "RB-CONF $rb_disk"
	fi
else
	echo "RB-NODISK ${rb_disk:-none}"
fi

# The suite scripts read the settings from the environment. They run "$RB_SAVE" to write the
# results so far, for example after each test directory.
RB_SAVE="$0 save"
export RB_SAVE RB_CONF RB_WORK RB_LIBEXEC
export rb_suites rb_disk rb_smoke_package rb_smoke_module rb_smoke_spare rb_pkg_dir
export rb_tests_only rb_tests_except rb_kyua_parallelism rb_modules_only rb_modules_skip
export rb_bench_scale rb_bench_only rb_foreign_timeout

uname -a > "$out/uname.txt"
if [ -r /var/run/dmesg.boot ]; then
	cp /var/run/dmesg.boot "$out/dmesg.boot"
else
	dmesg > "$out/dmesg.boot" 2>&1
fi
: > "$out/suites"
save

done_line=
for suite in $rb_suites; do
	echo "RB-SUITE $suite"
	case $suite in
	boot)
		result=pass
		;;
	smoke | tests | modules | bench | foreign)
		"$libexec/$suite" "$out/$suite" 2>&1 | tee "$out/$suite.log"
		result=$(counts "$out/$suite")
		;;
	*)
		result=unknown
		;;
	esac
	echo "$suite $result" >> "$out/suites"
	echo "RB-SUITE-END $suite $result"
	done_line="$done_line $suite=$result"
	save
done

dmesg > "$out/dmesg.end" 2>&1
# What the suites left behind. A process or a mount that a test did not remove can stop the
# shutdown.
ps -axww -o pid,ppid,stat,wchan,etime,command > "$out/ps.end" 2>&1
mount > "$out/mount.end" 2>&1
if save; then
	archive=disk
else
	archive=console
	(cd "$out" && tar -cf - .) | uuencode -m rb.tar
fi
echo "RB-DONE$done_line archive=$archive"

# A test can leave something behind that stops rc.shutdown or the sync, and then the VM does
# not power off. The results are on the results disk already, so after rb_poweroff_wait seconds
# this child powers off with halt -n (no sync) and -q (no rc.shutdown, no signals). It ignores
# SIGTERM, so it lives until halt sends SIGKILL to all processes. It writes to /dev/console
# because the output of /etc/rc goes through a pipe that the shutdown can stop. The processes
# and the mounts go to the console in the background, because ps or mount can also hang.
poweroff_watchdog()
{
	trap '' HUP TERM
	sleep "$rb_poweroff_wait"
	exec > /dev/console 2>&1
	echo "RB-POWEROFF-FORCE $rb_poweroff_wait"
	(
		ps -axww -o pid,ppid,stat,wchan,etime,command | sed 's/^/RB-PS /'
		mount | sed 's/^/RB-MOUNT /'
	) &
	sleep 5
	halt -l -n -p -q
}

# NetBSD's shutdown forks, and the child waits some seconds before it powers off. In that time
# /etc/rc continues and prints a login prompt, which rb boot reads as no power off. -D keeps
# shutdown in the foreground, so /etc/rc stops here until the VM powers off.
case $rb_poweroff in
[Yy][Ee][Ss])
	poweroff_watchdog &
	sync
	case $(uname -s) in
	NetBSD) shutdown -D -p now ;;
	*) shutdown -p now ;;
	esac
	;;
*)
	sync
	;;
esac
exit 0
