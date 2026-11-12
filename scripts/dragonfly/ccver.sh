# shellcheck shell=sh
# The make variables that give the CCVER of rucc the configuration of the reference CCVER (plan
# 05.3 and open question 10). build.sh, image.sh and mkdiff.sh source this file.
#
# share/mk and sys/conf test the name of CCVER in two ways:
# - with patterns, such as ${CCVER:Mgcc*} and ${_WCCVER:Mgcc8*}, for the GCC warning sets and the
#   kernel code flags. A CCVER that starts with the name of the reference, such as gcc80rucc for
#   gcc80, matches the same patterns.
# - with equality to gcc80 (gcc120 on master), for three things: the CPU flags of bsd.cpu.mk,
#   -Werror in bsd.sys.mk, and WERROR in kern.pre.mk and kmod.mk. Three variables on the make
#   command line give these the values of the reference: CCVER_BSD_CPU_MK, _WCCVER and WERROR.
#   make passes them to each make below it. WERROR is the value that kmod.mk gives with CCVER set
#   to the reference, so NO_WERROR still turns it off.
#
# RB_CCVER is the CCVER of rucc, RB_CCVER_REF the reference. With RB_CCVER_REF empty, or equal to
# RB_CCVER, there are no such variables.
rb_ccver_make() {
	if [ -z "${RB_CCVER_REF:-}" ] || [ "$RB_CCVER_REF" = "${RB_CCVER:-}" ]; then
		return 0
	fi
	rb_werror=$(cd "$RB_SRC/sys/vfs/ext2fs" && env CCVER="$RB_CCVER_REF" HOST_CCVER="$RB_CCVER_REF" \
		make -m "$RB_SRC/share/mk" __MAKE_CONF=/dev/null -V WERROR 2>/dev/null || true)
	echo "CCVER_BSD_CPU_MK=bsd.cpu.$RB_CCVER_REF.mk _WCCVER=$RB_CCVER_REF${rb_werror:+ WERROR=$rb_werror}"
}
