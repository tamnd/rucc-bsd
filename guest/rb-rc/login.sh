#!/bin/sh
# rb-rc's start with rb boot --login (plan 12.3), on an image that has no rb-rc, such as a release
# image of the tcg cadence. rb logs in as root, unpacks the results disk into /tmp and runs this
# script as /tmp/rb/login. The other scripts of rb-rc are beside it.
#
# On NetBSD, init raises kern.securelevel from 0 to 1 when /etc/rc is done. So the shell after
# the login is at level 1, and modload, which the smoke and modules suites use, fails with EPERM.
# Only init can lower the level, so only a new boot makes it 0 again. At a level above 0 on
# NetBSD, the script thus installs rb-rc as rb image does, and reboots:
#   - the scripts in /usr/libexec/rb and the rc script as /etc/rc.d/rb;
#   - rb_enable=YES and securelevel=-1 in /etc/rc.conf. With securelevel=-1, /etc/rc.d/securelevel
#     says that it cannot lower the level and keeps it at 0;
#   - kern.heartbeat.max_period=60 in /etc/sysctl.conf. The heartbeat check panics the kernel
#     ("softints stuck for 16 seconds") when the uptime of a CPU goes on by more than 15 s
#     between two of its clock interrupts. Under TCG on a busy runner, QEMU can stop a vCPU for
#     that long, and the stock G kernel of netbsd-aarch64 panicked so in weekly run 43251716464,
#     once in a test and once in /etc/rc (tamnd/rucc-bsd#206). 60 s keeps the check for a real
#     hang.
# It prints "RB-LOGIN-REBOOT <level>" before the reboot, and rb boot starts QEMU again once for
# it. At the next boot, rb-rc runs from /etc/rc at level 0, before init raises the level, and
# reads the rb.conf on the results disk as usual.
#
# Else, as on FreeBSD, where the level stays -1, it runs rb-rc from /tmp/rb at once.
#
# The host tests run this script with no VM. RB_LOGIN_ROOT puts the installed files under another
# directory, and stubs of uname, sysctl, sync and reboot come first in PATH.

here=$(cd "$(dirname "$0")" && pwd)
root=${RB_LOGIN_ROOT:-}

level=$(sysctl -n kern.securelevel 2> /dev/null)
case $level in
'' | *[!0-9-]*) level=0 ;;
esac

if [ "$(uname -s)" = NetBSD ] && [ "$level" -gt 0 ]; then
	mkdir -p "$root/usr/libexec/rb" "$root/etc/rc.d" || exit 1
	for f in run modules bench foreign smoke tests; do
		cp "$here/$f" "$root/usr/libexec/rb/$f" || exit 1
		chmod 555 "$root/usr/libexec/rb/$f"
	done
	cp "$here/rc" "$root/etc/rc.d/rb" || exit 1
	chmod 555 "$root/etc/rc.d/rb"
	printf 'rb_enable="YES"\nsecurelevel=-1\n' >> "$root/etc/rc.conf" || exit 1
	echo 'kern.heartbeat.max_period=60' >> "$root/etc/sysctl.conf" || exit 1
	echo "RB-NOTE login kern.heartbeat.max_period=60: QEMU under TCG can stop a vCPU for more than 15 s (tamnd/rucc-bsd#206)"
	echo "RB-LOGIN-REBOOT $level"
	sync
	exec reboot
fi

RB_LIBEXEC=$here
export RB_LIBEXEC
exec sh "$here/run"
