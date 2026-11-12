/*
 * The four TLS models. MODE picks the build:
 *   1  local-exec: the variables are in this unit;
 *   2  initial-exec: the variables are in the unit built with MODE 0;
 *   3  global-dynamic and 4 local-dynamic: the variables are in rb-tls.so, built with MODE 5 or 6,
 *      which main opens with dlopen, so ld.elf_so serves each access through __tls_get_addr.
 * Two threads and main each run the same work on their own copy.
 * Prints "19557 24117 7 10 1 3 21599".
 */
#include <dlfcn.h>
#include <pthread.h>
#include <stdio.h>

#if MODE == 1
#define STORAGE static
#define MODEL __attribute__((tls_model("local-exec")))
#elif MODE == 2
#define STORAGE extern
#define MODEL __attribute__((tls_model("initial-exec")))
#elif MODE == 5
#define STORAGE
#define MODEL __attribute__((tls_model("global-dynamic")))
#elif MODE == 6
#define STORAGE static
#define MODEL __attribute__((tls_model("local-dynamic")))
#else
#define STORAGE
#define MODEL
#endif

#if MODE == 2
extern __thread unsigned int tls_a MODEL;
extern __thread unsigned int tls_b[4] MODEL;
extern __thread unsigned int tls_zero MODEL;
extern __thread unsigned char tls_big[64] MODEL;
#elif MODE != 3 && MODE != 4
STORAGE __thread unsigned int tls_a MODEL = 7u;
STORAGE __thread unsigned int tls_b[4] MODEL = {1u, 2u, 3u, 4u};
STORAGE __thread unsigned int tls_zero MODEL;
STORAGE __thread unsigned char tls_big[64] MODEL __attribute__((aligned(64)));
#endif

#if MODE == 1 || MODE == 2 || MODE == 5 || MODE == 6
__attribute__((noinline)) unsigned int
rb_work(unsigned int seed)
{
	for (unsigned int i = 0; i < 100u; i++) {
		tls_a += seed + i;
		tls_b[i & 3u] ^= i * seed;
		tls_big[i & 63u] += (unsigned char)i;
		tls_zero += tls_big[(i * 7u) & 63u];
	}
	return tls_a * 3u + tls_b[0] + tls_b[1] + tls_b[2] + tls_b[3] + tls_zero;
}

unsigned long rb_addr(void) { return (unsigned long)&tls_a; }
unsigned int rb_aligned(void) { return ((unsigned long)tls_big & 63u) == 0u; }
unsigned int rb_a(void) { return tls_a; }
unsigned int rb_rest(void) { return tls_b[0] + tls_b[1] + tls_b[2] + tls_b[3] + tls_zero; }
#endif

#if MODE >= 1 && MODE <= 4
struct slot {
	unsigned int seed, value, aligned;
	unsigned long addr;
};

static unsigned int (*p_work)(unsigned int);
static unsigned long (*p_addr)(void);
static unsigned int (*p_aligned)(void);
static unsigned int (*p_a)(void);
static unsigned int (*p_rest)(void);

static void *
thread_main(void *arg)
{
	struct slot *s = arg;

	s->value = p_work(s->seed);
	s->addr = p_addr();
	s->aligned = p_aligned();
	return 0;
}

int
main(void)
{
	struct slot slots[2] = {{3u, 0u, 0u, 0ul}, {11u, 0u, 0u, 0ul}};
	pthread_t threads[2];

#if MODE == 3 || MODE == 4
	void *h = dlopen("./rb-tls.so", RTLD_NOW);
	if (h == NULL) {
		printf("dlopen: %s\n", dlerror());
		return 1;
	}
	*(void **)&p_work = dlsym(h, "rb_work");
	*(void **)&p_addr = dlsym(h, "rb_addr");
	*(void **)&p_aligned = dlsym(h, "rb_aligned");
	*(void **)&p_a = dlsym(h, "rb_a");
	*(void **)&p_rest = dlsym(h, "rb_rest");
#else
	p_work = rb_work;
	p_addr = rb_addr;
	p_aligned = rb_aligned;
	p_a = rb_a;
	p_rest = rb_rest;
#endif
	for (int i = 0; i < 2; i++)
		pthread_create(&threads[i], 0, thread_main, &slots[i]);
	for (int i = 0; i < 2; i++)
		pthread_join(threads[i], 0);
	unsigned int a = p_a(), rest = p_rest();
	unsigned int distinct = slots[0].addr != slots[1].addr &&
	    slots[0].addr != p_addr() && slots[1].addr != p_addr();
	unsigned int aligned = slots[0].aligned + slots[1].aligned + p_aligned();
	unsigned int mine = p_work(6u);
	printf("%u %u %u %u %u %u %u\n", slots[0].value, slots[1].value, a, rest, distinct,
	    aligned, mine);
	return 0;
}
#endif
