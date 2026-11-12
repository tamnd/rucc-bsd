/*
 * An ifunc in a static program. The linker puts an R_X86_64_IRELATIVE relocation between
 * __rela_iplt_start and __rela_iplt_end, and crt0 (lib/csu/common/crt0-common.c) must run the
 * resolver before main. Prints "1 42 42".
 */
#include <stdio.h>

static int resolved;

static int
add_impl(int a, int b)
{
	return a + b;
}

static int (*resolve_add(void))(int, int)
{
	resolved = 1;
	return add_impl;
}

int rb_add(int, int) __attribute__((ifunc("resolve_add")));

int (*volatile rb_add_ptr)(int, int) = rb_add;

int
main(void)
{
	printf("%d %d %d\n", resolved, rb_add(40, 2), rb_add_ptr(41, 1));
	return 0;
}
