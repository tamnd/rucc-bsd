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

network()
{
	ifconfig vtnet0 &&
	ifconfig vtnet0 inet | grep -q 'inet 10\.0\.2\.' &&
	netstat -rn -f inet | grep -q '^default'
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

tests_present()
{
	[ -f /usr/tests/Kyuafile ] && kyua about version
}

check shells shells
check network network
check fsck-mount fsck_mount
check pkg-local pkg_local
check module module
check tests-present tests_present

passed=$(grep -c '^pass ' "$out/results")
total=$(wc -l < "$out/results" | tr -d ' ')
echo "$passed/$total" > "$out/counts"
