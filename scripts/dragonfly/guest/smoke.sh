#!/bin/sh
# The smoke suite for DragonFly (plan 02.3 part 3 and 12.2), as the smoke.sh of
# scripts/freebsd/guest. The same script runs on R and G.
#
#   smoke.sh OUTDIR
#
# Each check writes one line "pass NAME" or "fail NAME" to OUTDIR/results, and its output to
# OUTDIR/NAME.log. OUTDIR/counts gets "PASSED/TOTAL". RB_SMOKE_DISK names an empty disk that the
# fsck check may destroy.
#
# The differences from FreeBSD: the disk check uses fdisk and disklabel as nrelease does, as
# DragonFly has no gpart. fsck is the fsck of UFS. The module check loads ext2fs, which
# X86_64_GENERIC does not have in the kernel. The static program comes from /rescue, which has gzip
# and pax but no tar. DragonFly has no kyua and no /usr/tests, so there is no tests check.
out=$1
mkdir -p "$out"
: > "$out/results"
: "${RB_SMOKE_DISK:=da2}"

check()
{
	local name=$1
	shift
	if "$@" > "$out/$name.log" 2>&1; then
		echo "pass $name" >> "$out/results"
	else
		echo "fail $name" >> "$out/results"
	fi
}

shells()
{
	[ "$(/bin/sh -c 'echo $((6 * 7))')" = 42 ] &&
	[ "$(/bin/csh -c 'echo ok')" = ok ] &&
	[ "$(/bin/tcsh -c 'echo ok')" = ok ]
}

# QEMU's user network runs with restrict=on, so its DHCP server gives no router and there is no
# default route. The virtual host 10.0.2.2 answers ping in that mode, so the check pings it.
# The link of em0 can come up after rc has started dhclient, so the check waits up to 60 seconds
# for the address and starts dhclient again while there is none.
network()
{
	local i
	for i in 1 2 3 4 5 6; do
		ifconfig em0 inet | grep -q 'inet 10\.0\.2\.' && break
		ifconfig em0 | grep 'status:'
		dhclient em0
		sleep 10
	done
	ifconfig em0 &&
	ifconfig em0 inet | grep -q 'inet 10\.0\.2\.' &&
	netstat -rn -f inet &&
	ping -c 3 -t 10 10.0.2.2
}

fsck_mount()
{
	local d=${RB_SMOKE_DISK} m=/var/rb/smoke-mnt l=/var/rb/smoke-label
	fdisk -IB ${d} &&
	disklabel -w -r ${d}s1 auto &&
	disklabel -r ${d}s1 > ${l} &&
	echo "a: * * 4.2BSD" >> ${l} &&
	disklabel -R -r ${d}s1 ${l} &&
	newfs /dev/${d}s1a &&
	fsck -y /dev/${d}s1a &&
	mkdir -p ${m} &&
	mount /dev/${d}s1a ${m} &&
	echo rb > ${m}/file &&
	[ "$(cat ${m}/file)" = rb ] &&
	umount ${m} &&
	fsck -y /dev/${d}s1a &&
	mount -o ro /dev/${d}s1a ${m} &&
	[ "$(cat ${m}/file)" = rb ] &&
	umount ${m}
}

pkg_local()
{
	env REPOS_DIR=/usr/local/etc/rb-pkg ASSUME_ALWAYS_YES=yes \
		pkg install -y -r rb tree &&
	/usr/local/bin/tree -L 1 / &&
	env ASSUME_ALWAYS_YES=yes pkg delete -y tree
}

module()
{
	! kldstat -q -m ext2fs &&
	kldload ext2fs &&
	kldstat -q -m ext2fs &&
	lsvfs ext2fs &&
	kldunload ext2fs &&
	! kldstat -q -m ext2fs &&
	mkdir -p /var/rb/null &&
	mount_null /usr/share /var/rb/null &&
	[ -d /var/rb/null/misc ] &&
	umount /var/rb/null
}

# A static program and a dynamic one (plan 09.4). /rescue is one static program. Both come from one
# source, so their output must be the same.
static_dynamic()
{
	local d=/var/rb/static-dynamic
	file /rescue/gzip /usr/bin/gzip &&
	ldd /usr/bin/gzip | grep -q 'libc\.so' &&
	! ldd /rescue/gzip 2> /dev/null | grep -q 'libc\.so' &&
	mkdir -p ${d} &&
	jot 50000 > ${d}/in &&
	/rescue/gzip -c -n ${d}/in > ${d}/static.gz &&
	/usr/bin/gzip -c -n ${d}/in > ${d}/dynamic.gz &&
	cmp ${d}/static.gz ${d}/dynamic.gz &&
	/rescue/gzip -d -c ${d}/dynamic.gz | cmp - ${d}/in &&
	/usr/bin/gzip -d -c ${d}/static.gz | cmp - ${d}/in &&
	(cd ${d} && /rescue/pax -w -f static.pax in) &&
	(cd ${d} && mkdir out && cd out && /bin/pax -r -f ../static.pax) &&
	cmp ${d}/out/in ${d}/in &&
	[ "$(/rescue/sh -c 'echo $((6 * 7))')" = 42 ] &&
	rm -rf ${d}
}

check shells shells
check network network
check fsck-mount fsck_mount
check pkg-local pkg_local
check module module
check static-dynamic static_dynamic

passed=$(grep -c '^pass ' "$out/results")
total=$(wc -l < "$out/results" | tr -d ' ')
echo "$passed/$total" > "$out/counts"
