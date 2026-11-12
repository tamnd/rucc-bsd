#!/bin/sh
# The smoke suite (plan 02.3 part 3 and 12.2). The same script runs on R and G.
#
#   smoke.sh OUTDIR
#
# Each check writes one line "pass NAME" or "fail NAME" to OUTDIR/results, and its output to
# OUTDIR/NAME.log. OUTDIR/counts gets "PASSED/TOTAL". RB_SMOKE_DISK names an empty disk that the
# fsck check may destroy.
out=$1
mkdir -p "$out"
: > "$out/results"
: "${RB_SMOKE_DISK:=vtbd7}"

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
network()
{
	ifconfig vtnet0 &&
	ifconfig vtnet0 inet | grep -q 'inet 10\.0\.2\.' &&
	netstat -rn -f inet &&
	ping -c 3 -t 10 10.0.2.2
}

fsck_mount()
{
	local d=/dev/${RB_SMOKE_DISK} m=/var/rb/smoke-mnt
	gpart create -s gpt ${RB_SMOKE_DISK} &&
	gpart add -t freebsd-ufs -l rbsmoke ${RB_SMOKE_DISK} &&
	newfs -U ${d}p1 &&
	fsck -t ufs -y ${d}p1 &&
	mkdir -p ${m} &&
	mount ${d}p1 ${m} &&
	echo rb > ${m}/file &&
	[ "$(cat ${m}/file)" = rb ] &&
	umount ${m} &&
	fsck -t ufs -y ${d}p1 &&
	mount -o ro ${d}p1 ${m} &&
	[ "$(cat ${m}/file)" = rb ] &&
	umount ${m}
}

pkg_local()
{
	env REPOS_DIR=/usr/local/etc/rb-pkg IGNORE_OSVERSION=yes ASSUME_ALWAYS_YES=yes \
		pkg install -y -r rb tree &&
	/usr/local/bin/tree -L 1 / &&
	env ASSUME_ALWAYS_YES=yes pkg delete -y tree
}

module()
{
	! kldstat -q -m nullfs &&
	kldload nullfs &&
	kldstat -q -m nullfs &&
	mkdir -p /var/rb/null &&
	mount -t nullfs /usr/share /var/rb/null &&
	[ -d /var/rb/null/misc ] &&
	umount /var/rb/null &&
	kldunload nullfs &&
	! kldstat -q -m nullfs
}

# A static program and a dynamic one (plan 09.4). /rescue is one static program, so the ifunc
# resolvers of libc.a run through IRELATIVE before main. /usr/bin has dynamic programs, so those of
# libc.so.7 run in rtld. gzip and tar use the memcpy family. Both programs come from one source, so
# their output must be the same.
static_dynamic()
{
	local d=/var/rb/static-dynamic
	file -b /rescue/gzip | grep 'statically linked' &&
	file -b /usr/bin/gzip | grep 'dynamically linked' &&
	mkdir -p ${d} &&
	jot 50000 > ${d}/in &&
	/rescue/gzip -c -n ${d}/in > ${d}/static.gz &&
	/usr/bin/gzip -c -n ${d}/in > ${d}/dynamic.gz &&
	cmp ${d}/static.gz ${d}/dynamic.gz &&
	/rescue/gzip -d -c ${d}/dynamic.gz | cmp - ${d}/in &&
	/usr/bin/gzip -d -c ${d}/static.gz | cmp - ${d}/in &&
	/rescue/tar -cf ${d}/static.tar -C ${d} in &&
	/usr/bin/tar -xOf ${d}/static.tar in | cmp - ${d}/in &&
	[ "$(/rescue/sh -c 'echo $((6 * 7))')" = 42 ] &&
	rm -rf ${d}
}

tests_present()
{
	[ -f /usr/tests/Kyuafile ] && kyua about version
}

check shells shells
check network network
check fsck-mount fsck_mount
check pkg-local pkg_local
check module module
check static-dynamic static_dynamic
check tests-present tests_present

passed=$(grep -c '^pass ' "$out/results")
total=$(wc -l < "$out/results" | tr -d ' ')
echo "$passed/$total" > "$out/counts"
