/*
 * rb-bench: the C benchmarks of the B8 benchmark set (plan 13.2).
 *
 * The bench suite of rb-rc (guest/rb-rc/bench.sh) runs this program in the guest. Each image runs
 * the copy that its own compiler built, so the code of this program is graded with the system.
 *
 *   rb-bench [-s SCALE] [NAME ...]          run the benchmarks, all or the ones named
 *   rb-bench -l                             list the benchmarks: group and name
 *   rb-bench -V                             print the version
 *   rb-bench -x GROUP NAME CMD [ARG ...]    run CMD and print its wall time as GROUP NAME
 *   rb-bench -g log FILE MIB                write a fixed text log of MIB MiB
 *   rb-bench -g lines FILE N                write N fixed lines of words, for sort
 *   rb-bench -g tree DIR N                  write N small files in DIR, 100 in each subdirectory
 *
 * Each benchmark does a fixed amount of work and prints one line: the group, the name, the wall
 * time in seconds and "lower", separated by tabs. SCALE multiplies the work (default 1). The
 * host tests use a small SCALE. The work is fixed for a major version of rb-bench, so that the
 * results of two weeks can be compared. A benchmark that cannot run prints nothing, writes a
 * line on stderr, and makes the exit status 1.
 *
 * The data is from a fixed pseudo random sequence, so both sides do the same work.
 */

#include <sys/types.h>
#if defined(__FreeBSD__) || defined(__NetBSD__) || defined(__APPLE__)
#define HAVE_KQUEUE 1
#include <sys/event.h>
#endif
#include <sys/mman.h>
#include <sys/resource.h>
#include <sys/select.h>
#include <sys/socket.h>
#include <sys/stat.h>
#include <sys/time.h>
#include <sys/wait.h>

#include <netinet/in.h>
#include <netinet/tcp.h>
#include <arpa/inet.h>

#include <errno.h>
#include <fcntl.h>
#include <math.h>
#include <poll.h>
#include <pthread.h>
#include <regex.h>
#include <signal.h>
#include <spawn.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <time.h>
#include <unistd.h>

#define VERSION "1"

extern char **environ;

static double scale = 1.0;
static volatile uint64_t sink;

static double
now(void)
{
	struct timespec ts;

	clock_gettime(CLOCK_MONOTONIC, &ts);
	return (double)ts.tv_sec + (double)ts.tv_nsec / 1e9;
}

/* The work of a benchmark at scale 1, times SCALE, and at least 1. */
static uint64_t
work(uint64_t n)
{
	double w = (double)n * scale;

	return w < 1.0 ? 1 : (uint64_t)w;
}

/* xorshift64: the fixed sequence of the data. */
static uint64_t rng_state = 0x9e3779b97f4a7c15ULL;

static uint64_t
rng(void)
{
	uint64_t x = rng_state;

	x ^= x << 13;
	x ^= x >> 7;
	x ^= x << 17;
	return rng_state = x;
}

static void
fail(const char *name, const char *what)
{
	fprintf(stderr, "rb-bench: %s: %s: %s\n", name, what, strerror(errno));
}

/* System calls. */

static int
b_getpid(void)
{
	uint64_t i, n = work(5000000);

	for (i = 0; i < n; i++)
		sink += (uint64_t)getpid();
	return 0;
}

static int
b_pipe_rw1(void)
{
	uint64_t i, n = work(1000000);
	int p[2];
	char c = 'x';

	if (pipe(p) == -1)
		return -1;
	for (i = 0; i < n; i++) {
		if (write(p[1], &c, 1) != 1 || read(p[0], &c, 1) != 1)
			return -1;
	}
	close(p[0]);
	close(p[1]);
	return 0;
}

static int
b_open_close(void)
{
	uint64_t i, n = work(1000000);
	int fd;

	for (i = 0; i < n; i++) {
		if ((fd = open("/etc/passwd", O_RDONLY)) == -1)
			return -1;
		close(fd);
	}
	return 0;
}

static int
b_stat(void)
{
	uint64_t i, n = work(2000000);
	struct stat st;

	for (i = 0; i < n; i++) {
		if (stat("/etc/passwd", &st) == -1)
			return -1;
		sink += (uint64_t)st.st_size;
	}
	return 0;
}

static int
b_gettimeofday(void)
{
	uint64_t i, n = work(20000000);
	struct timeval tv;

	for (i = 0; i < n; i++) {
		gettimeofday(&tv, NULL);
		sink += (uint64_t)tv.tv_usec;
	}
	return 0;
}

static int
b_clock_gettime(void)
{
	uint64_t i, n = work(20000000);
	struct timespec ts;

	for (i = 0; i < n; i++) {
		clock_gettime(CLOCK_MONOTONIC, &ts);
		sink += (uint64_t)ts.tv_nsec;
	}
	return 0;
}

/* Processes. */

static int
b_fork_wait(void)
{
	uint64_t i, n = work(20000);
	pid_t pid;
	int status;

	for (i = 0; i < n; i++) {
		if ((pid = fork()) == -1)
			return -1;
		if (pid == 0)
			_exit(0);
		if (waitpid(pid, &status, 0) == -1)
			return -1;
	}
	return 0;
}

/* One vfork and exec of true. In a function of its own, so the loop's variables are not live
 * across vfork. */
static int
vfork_true(void)
{
	pid_t pid;
	int status;

	if ((pid = vfork()) == -1)
		return -1;
	if (pid == 0) {
		execl("/usr/bin/true", "true", (char *)NULL);
		_exit(127);
	}
	if (waitpid(pid, &status, 0) == -1 || status != 0)
		return -1;
	return 0;
}

static int
b_vfork_exec(void)
{
	uint64_t i, n = work(5000);

	for (i = 0; i < n; i++) {
		if (vfork_true() != 0)
			return -1;
	}
	return 0;
}

static int
b_posix_spawn(void)
{
	uint64_t i, n = work(5000);
	char *argv[] = { "true", NULL };
	pid_t pid;
	int status;

	for (i = 0; i < n; i++) {
		if (posix_spawn(&pid, "/usr/bin/true", NULL, NULL, argv, environ) != 0)
			return -1;
		if (waitpid(pid, &status, 0) == -1 || status != 0)
			return -1;
	}
	return 0;
}

/* Ping-pong of one byte over two pipes, n round trips. */
static int
pingpong(int in, int out, uint64_t n)
{
	uint64_t i;
	char c;

	for (i = 0; i < n; i++) {
		if (read(in, &c, 1) != 1 || write(out, &c, 1) != 1)
			return -1;
	}
	return 0;
}

struct pp {
	int in, out;
	uint64_t n;
};

static void *
pingpong_thread(void *arg)
{
	struct pp *pp = arg;

	pingpong(pp->in, pp->out, pp->n);
	return NULL;
}

static int
ctx(int threads)
{
	uint64_t i, n = work(500000);
	int a[2], b[2], rc = 0;
	pthread_t t;
	pid_t pid = -1;
	struct pp pp;
	char c = 'x';

	if (pipe(a) == -1 || pipe(b) == -1)
		return -1;
	pp.in = a[0];
	pp.out = b[1];
	pp.n = n;
	if (threads) {
		if (pthread_create(&t, NULL, pingpong_thread, &pp) != 0)
			return -1;
	} else {
		if ((pid = fork()) == -1)
			return -1;
		if (pid == 0)
			_exit(pingpong(a[0], b[1], n) == 0 ? 0 : 1);
	}
	for (i = 0; i < n; i++) {
		if (write(a[1], &c, 1) != 1 || read(b[0], &c, 1) != 1) {
			rc = -1;
			break;
		}
	}
	if (threads)
		pthread_join(t, NULL);
	else
		waitpid(pid, NULL, 0);
	close(a[0]);
	close(a[1]);
	close(b[0]);
	close(b[1]);
	return rc;
}

static int
b_ctx_proc(void)
{
	return ctx(0);
}

static int
b_ctx_thread(void)
{
	return ctx(1);
}

/* Memory. */

static int
b_pagefault(void)
{
	size_t size = (size_t)work(1 << 30), page = (size_t)getpagesize(), off;
	uint64_t r, rounds = 4;
	char *p;

	if (size < page)
		size = page;
	for (r = 0; r < rounds; r++) {
		p = mmap(NULL, size, PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE, -1, 0);
		if (p == MAP_FAILED)
			return -1;
		for (off = 0; off < size; off += page)
			p[off] = 1;
		munmap(p, size);
	}
	return 0;
}

static int
b_mmap_64k(void)
{
	uint64_t i, n = work(500000);
	void *p;

	for (i = 0; i < n; i++) {
		p = mmap(NULL, 65536, PROT_READ | PROT_WRITE, MAP_ANON | MAP_PRIVATE, -1, 0);
		if (p == MAP_FAILED)
			return -1;
		munmap(p, 65536);
	}
	return 0;
}

#define SLOTS 1024

/* Allocations of 16 bytes to 1 MiB: the size class k has the chance 2^-(k+1). */
static void *
malloc_mix(void *arg)
{
	uint64_t i, n = work(5000000), x = (uint64_t)(uintptr_t)arg;
	char *slot[SLOTS] = { 0 };
	size_t size;
	unsigned k;
	int j;

	for (i = 0; i < n; i++) {
		x ^= x << 13;
		x ^= x >> 7;
		x ^= x << 17;
		j = (int)(x % SLOTS);
		free(slot[j]);
		k = 0;
		while (k < 16 && ((x >> (10 + k)) & 1))
			k++;
		size = (size_t)16 << k;
		if ((slot[j] = malloc(size)) == NULL)
			return (void *)1;
		slot[j][0] = (char)i;
	}
	for (j = 0; j < SLOTS; j++)
		free(slot[j]);
	return NULL;
}

static int
b_malloc_1t(void)
{
	return malloc_mix((void *)(uintptr_t)0x1234567) == NULL ? 0 : -1;
}

static int
threads(void *(*fn)(void *), int count)
{
	pthread_t t[8];
	void *res;
	int i, rc = 0;

	for (i = 0; i < count; i++) {
		if (pthread_create(&t[i], NULL, fn, (void *)(uintptr_t)(0x1234567 + i)) != 0)
			return -1;
	}
	for (i = 0; i < count; i++) {
		pthread_join(t[i], &res);
		if (res != NULL)
			rc = -1;
	}
	return rc;
}

static int
b_malloc_4t(void)
{
	return threads(malloc_mix, 4);
}

/* Events: one pipe of many is ready each time. */

static int
pipes(int (*p)[2], int count)
{
	int i;

	for (i = 0; i < count; i++) {
		if (pipe(p[i]) == -1)
			return -1;
	}
	return 0;
}

static void
close_pipes(int (*p)[2], int count)
{
	int i;

	for (i = 0; i < count; i++) {
		close(p[i][0]);
		close(p[i][1]);
	}
}

#ifdef HAVE_KQUEUE
static int
more_files(int want)
{
	struct rlimit rl;

	if (getrlimit(RLIMIT_NOFILE, &rl) == -1)
		return -1;
	if (rl.rlim_cur < (rlim_t)want) {
		rl.rlim_cur = rl.rlim_max < (rlim_t)want ? rl.rlim_max : (rlim_t)want;
		if (setrlimit(RLIMIT_NOFILE, &rl) == -1)
			return -1;
	}
	return rl.rlim_cur >= (rlim_t)want ? 0 : -1;
}

static int
b_kqueue_1000(void)
{
	static int p[1000][2];
	struct kevent ev;
	uint64_t i, n = work(500000);
	int kq, j, rc = 0;
	char c = 'x';

	if (more_files(2100) == -1 || pipes(p, 1000) == -1 || (kq = kqueue()) == -1)
		return -1;
	for (j = 0; j < 1000; j++) {
		EV_SET(&ev, p[j][0], EVFILT_READ, EV_ADD, 0, 0, NULL);
		if (kevent(kq, &ev, 1, NULL, 0, NULL) == -1)
			return -1;
	}
	for (i = 0; i < n && rc == 0; i++) {
		j = (int)(i * 7 % 1000);
		if (write(p[j][1], &c, 1) != 1 || kevent(kq, NULL, 0, &ev, 1, NULL) != 1 ||
		    read((int)ev.ident, &c, 1) != 1)
			rc = -1;
	}
	close(kq);
	close_pipes(p, 1000);
	return rc;
}
#endif

static int
b_poll_100(void)
{
	int p[100][2], j, rc = 0;
	struct pollfd fds[100];
	uint64_t i, n = work(500000);
	char c = 'x';

	if (pipes(p, 100) == -1)
		return -1;
	for (j = 0; j < 100; j++) {
		fds[j].fd = p[j][0];
		fds[j].events = POLLIN;
	}
	for (i = 0; i < n && rc == 0; i++) {
		j = (int)(i * 7 % 100);
		if (write(p[j][1], &c, 1) != 1 || poll(fds, 100, -1) != 1 ||
		    read(p[j][0], &c, 1) != 1)
			rc = -1;
	}
	close_pipes(p, 100);
	return rc;
}

static int
b_select_100(void)
{
	int p[100][2], j, max = 0, rc = 0;
	fd_set set;
	uint64_t i, n = work(500000);
	char c = 'x';

	if (pipes(p, 100) == -1)
		return -1;
	for (j = 0; j < 100; j++) {
		if (p[j][0] >= FD_SETSIZE)
			return -1;
		if (p[j][0] > max)
			max = p[j][0];
	}
	for (i = 0; i < n && rc == 0; i++) {
		j = (int)(i * 7 % 100);
		FD_ZERO(&set);
		for (int k = 0; k < 100; k++)
			FD_SET(p[k][0], &set);
		if (write(p[j][1], &c, 1) != 1 || select(max + 1, &set, NULL, NULL, NULL) != 1 ||
		    read(p[j][0], &c, 1) != 1)
			rc = -1;
	}
	close_pipes(p, 100);
	return rc;
}

/* Locking. */

static pthread_mutex_t mutex = PTHREAD_MUTEX_INITIALIZER;
static pthread_cond_t cond = PTHREAD_COND_INITIALIZER;
static pthread_rwlock_t rwlock = PTHREAD_RWLOCK_INITIALIZER;
static uint64_t counter;
static int turn;

static int
b_mutex(void)
{
	uint64_t i, n = work(50000000);

	for (i = 0; i < n; i++) {
		pthread_mutex_lock(&mutex);
		counter++;
		pthread_mutex_unlock(&mutex);
	}
	return 0;
}

static void *
mutex_loop(void *arg)
{
	uint64_t i, n = work(2500000);

	(void)arg;
	for (i = 0; i < n; i++) {
		pthread_mutex_lock(&mutex);
		counter++;
		pthread_mutex_unlock(&mutex);
	}
	return NULL;
}

static int
b_mutex_4t(void)
{
	return threads(mutex_loop, 4);
}

static void *
cond_side(void *arg)
{
	uint64_t i, n = work(200000);
	int me = (int)(uintptr_t)arg - 0x1234567;

	for (i = 0; i < n; i++) {
		pthread_mutex_lock(&mutex);
		while (turn != me)
			pthread_cond_wait(&cond, &mutex);
		turn = !me;
		pthread_cond_signal(&cond);
		pthread_mutex_unlock(&mutex);
	}
	return NULL;
}

static int
b_cond_pingpong(void)
{
	turn = 0;
	return threads(cond_side, 2);
}

static void *
rwlock_loop(void *arg)
{
	uint64_t i, n = work(5000000);

	(void)arg;
	for (i = 0; i < n; i++) {
		pthread_rwlock_rdlock(&rwlock);
		sink += counter;
		pthread_rwlock_unlock(&rwlock);
	}
	return NULL;
}

static int
b_rwlock_4t(void)
{
	return threads(rwlock_loop, 4);
}

/* Network, over 127.0.0.1. */

static int
listener(int type, struct sockaddr_in *sin)
{
	socklen_t len = sizeof(*sin);
	int s;

	memset(sin, 0, sizeof(*sin));
	sin->sin_family = AF_INET;
	sin->sin_addr.s_addr = htonl(INADDR_LOOPBACK);
	if ((s = socket(AF_INET, type, 0)) == -1)
		return -1;
	if (bind(s, (struct sockaddr *)sin, sizeof(*sin)) == -1 ||
	    getsockname(s, (struct sockaddr *)sin, &len) == -1 ||
	    (type == SOCK_STREAM && listen(s, 16) == -1)) {
		close(s);
		return -1;
	}
	return s;
}

struct stream {
	int fd;
	uint64_t bytes;
	int rc;
};

static void *
tcp_send(void *arg)
{
	struct stream *st = arg;
	static char buf[65536];
	uint64_t left = st->bytes;
	ssize_t w;

	while (left > 0) {
		w = write(st->fd, buf, left < sizeof(buf) ? (size_t)left : sizeof(buf));
		if (w <= 0) {
			st->rc = -1;
			break;
		}
		left -= (uint64_t)w;
	}
	shutdown(st->fd, SHUT_WR);
	return NULL;
}

static void *
tcp_recv(void *arg)
{
	struct stream *st = arg;
	char buf[65536];
	uint64_t got = 0;
	ssize_t r;

	while ((r = read(st->fd, buf, sizeof(buf))) > 0)
		got += (uint64_t)r;
	st->rc = got == st->bytes ? 0 : -1;
	return NULL;
}

/* COUNT streams over loopback, each of BYTES / COUNT bytes. */
static int
tcp(int count, uint64_t bytes)
{
	struct sockaddr_in sin;
	struct stream tx[8], rx[8];
	pthread_t t[16];
	int l, i, rc = 0;

	if ((l = listener(SOCK_STREAM, &sin)) == -1)
		return -1;
	for (i = 0; i < count; i++) {
		tx[i].bytes = rx[i].bytes = bytes / (uint64_t)count;
		tx[i].rc = rx[i].rc = 0;
		if ((tx[i].fd = socket(AF_INET, SOCK_STREAM, 0)) == -1 ||
		    connect(tx[i].fd, (struct sockaddr *)&sin, sizeof(sin)) == -1 ||
		    (rx[i].fd = accept(l, NULL, NULL)) == -1)
			return -1;
	}
	close(l);
	for (i = 0; i < count; i++) {
		pthread_create(&t[2 * i], NULL, tcp_send, &tx[i]);
		pthread_create(&t[2 * i + 1], NULL, tcp_recv, &rx[i]);
	}
	for (i = 0; i < 2 * count; i++)
		pthread_join(t[i], NULL);
	for (i = 0; i < count; i++) {
		if (tx[i].rc != 0 || rx[i].rc != 0)
			rc = -1;
		close(tx[i].fd);
		close(rx[i].fd);
	}
	return rc;
}

static int
b_tcp_1(void)
{
	return tcp(1, work(4ULL << 30));
}

static int
b_tcp_8(void)
{
	return tcp(8, work(4ULL << 30));
}

struct udp {
	int fd;
	uint64_t n;
};

static void *
udp_echo(void *arg)
{
	struct udp *u = arg;
	struct sockaddr_in from;
	socklen_t len;
	char buf[64];
	uint64_t i;

	for (i = 0; i < u->n; i++) {
		len = sizeof(from);
		if (recvfrom(u->fd, buf, sizeof(buf), 0, (struct sockaddr *)&from, &len) != 64 ||
		    sendto(u->fd, buf, 64, 0, (struct sockaddr *)&from, len) != 64)
			break;
	}
	return NULL;
}

/* Request and answer of 64 bytes. Loopback does not drop, so no timeouts are needed. */
static int
b_udp_64(void)
{
	struct sockaddr_in sin;
	struct udp u;
	pthread_t t;
	char buf[64] = { 0 };
	uint64_t i;
	int c, rc = 0;

	u.n = work(500000);
	if ((u.fd = listener(SOCK_DGRAM, &sin)) == -1 || (c = socket(AF_INET, SOCK_DGRAM, 0)) == -1)
		return -1;
	if (connect(c, (struct sockaddr *)&sin, sizeof(sin)) == -1)
		return -1;
	pthread_create(&t, NULL, udp_echo, &u);
	for (i = 0; i < u.n; i++) {
		if (send(c, buf, 64, 0) != 64 || recv(c, buf, 64, 0) != 64) {
			rc = -1;
			break;
		}
	}
	pthread_join(t, NULL);
	close(c);
	close(u.fd);
	return rc;
}

/* libc and libm. */

static int
cmp_int(const void *a, const void *b)
{
	int x = *(const int *)a, y = *(const int *)b;

	return (x > y) - (x < y);
}

static int
b_qsort(void)
{
	size_t i, n = (size_t)work(1000000);
	int r, *v = malloc(n * sizeof(*v));

	if (v == NULL)
		return -1;
	for (r = 0; r < 4; r++) {
		for (i = 0; i < n; i++)
			v[i] = (int)(rng() >> 33);
		qsort(v, n, sizeof(*v), cmp_int);
		sink += (uint64_t)v[n / 2];
	}
	free(v);
	return 0;
}

/* 1000 fixed doubles, from 1e-30 to 1e30. */
static double values[1000];

static void
make_values(void)
{
	int i;

	for (i = 0; i < 1000; i++) {
		double m = (double)(rng() >> 11) / 9007199254740992.0;
		int e = (int)(rng() % 61) - 30;

		values[i] = (i & 1 ? -m : m) * pow(10.0, e);
	}
}

static int
b_strtod(void)
{
	static char text[1000][32];
	uint64_t i, n = work(5000000);
	int j;

	for (j = 0; j < 1000; j++)
		snprintf(text[j], sizeof(text[j]), "%.17g", values[j]);
	for (i = 0; i < n; i++)
		sink += (uint64_t)(strtod(text[i % 1000], NULL) != 0.0);
	return 0;
}

static int
b_printf_g(void)
{
	uint64_t i, n = work(5000000);
	char buf[64];

	for (i = 0; i < n; i++)
		sink += (uint64_t)snprintf(buf, sizeof(buf), "%g", values[i % 1000]);
	return 0;
}

static int
b_regex(void)
{
	static const char *re[] = {
		"^[a-z]+[0-9]*$",
		"(foo|bar|baz)+qux",
		"[[:alpha:]][[:alnum:]_]*=[0-9]+",
		"a.*b.*c.*d",
	};
	static const char *text[] = {
		"hello42",
		"foobarbazfooqux and more",
		"   name_1=1234 ; other",
		"axxxbxxxcxxxdxxx",
		"no match here at all, none, nothing at all",
	};
	uint64_t i, n = work(100000);
	regex_t r;
	size_t k, t;

	for (i = 0; i < n; i++) {
		for (k = 0; k < sizeof(re) / sizeof(re[0]); k++) {
			if (regcomp(&r, re[k], REG_EXTENDED | REG_NOSUB) != 0)
				return -1;
			for (t = 0; t < sizeof(text) / sizeof(text[0]); t++)
				sink += (uint64_t)(regexec(&r, text[t], 0, NULL, 0) == 0);
			regfree(&r);
		}
	}
	return 0;
}

/* 16 sizes, 1 byte to 32 KiB, each moving the same number of bytes in all. */
static int
b_memcpy(void)
{
	static char a[32768], b[32768];
	uint64_t total = work(4ULL << 30), i, n;
	size_t size;

	for (size = 1; size <= sizeof(a); size <<= 1) {
		n = total / 16 / size;
		for (i = 0; i < n; i++) {
			memcpy(b, a + (i & 1), size - (i & 1));
			sink += (uint64_t)b[0];
		}
	}
	return 0;
}

static int
b_strlen(void)
{
	static char s[32769];
	uint64_t total = work(4ULL << 30), i, n;
	size_t size;

	memset(s, 'a', sizeof(s) - 1);
	for (size = 1; size <= sizeof(s) - 1; size <<= 1) {
		s[size] = '\0';
		n = total / 16 / size;
		for (i = 0; i < n; i++) {
			s[0] = (char)('a' + (i & 7));
			sink += strlen(s);
		}
		s[size] = 'a';
	}
	return 0;
}

static int
b_libm(void)
{
	uint64_t i, n = work(5000000);
	double x, sum = 0;

	for (i = 0; i < n; i++) {
		x = fabs(values[i % 1000]) * 1e-25 + 0.5;
		if (x > 700)
			x = 1.5;
		sum += sin(x) + cos(x) + exp(x) + log(x) + pow(x, 1.7) + sqrt(x) + atan2(x, 1.0);
	}
	sink += (uint64_t)sum;
	return 0;
}

static const struct bench {
	const char *name;
	const char *group;
	int (*fn)(void);
} benches[] = {
	{ "getpid", "syscalls", b_getpid },
	{ "pipe_rw1", "syscalls", b_pipe_rw1 },
	{ "open_close", "syscalls", b_open_close },
	{ "stat", "syscalls", b_stat },
	{ "gettimeofday", "syscalls", b_gettimeofday },
	{ "clock_gettime", "syscalls", b_clock_gettime },
	{ "fork_wait", "processes", b_fork_wait },
	{ "vfork_exec", "processes", b_vfork_exec },
	{ "posix_spawn", "processes", b_posix_spawn },
	{ "ctx_proc", "processes", b_ctx_proc },
	{ "ctx_thread", "processes", b_ctx_thread },
	{ "pagefault", "memory", b_pagefault },
	{ "mmap_64k", "memory", b_mmap_64k },
	{ "malloc_1t", "memory", b_malloc_1t },
	{ "malloc_4t", "memory", b_malloc_4t },
#ifdef HAVE_KQUEUE
	{ "kqueue_1000", "events", b_kqueue_1000 },
#endif
	{ "poll_100", "events", b_poll_100 },
	{ "select_100", "events", b_select_100 },
	{ "mutex", "locking", b_mutex },
	{ "mutex_4t", "locking", b_mutex_4t },
	{ "cond_pingpong", "locking", b_cond_pingpong },
	{ "rwlock_4t", "locking", b_rwlock_4t },
	{ "tcp_1", "network", b_tcp_1 },
	{ "tcp_8", "network", b_tcp_8 },
	{ "udp_64", "network", b_udp_64 },
	{ "qsort", "libc", b_qsort },
	{ "strtod", "libc", b_strtod },
	{ "printf_g", "libc", b_printf_g },
	{ "regex", "libc", b_regex },
	{ "memcpy", "libc", b_memcpy },
	{ "strlen", "libc", b_strlen },
	{ "libm", "libc", b_libm },
};

#define NBENCH (sizeof(benches) / sizeof(benches[0]))

static int
run(const struct bench *b)
{
	double t0, t1;

	rng_state = 0x9e3779b97f4a7c15ULL;
	make_values();
	t0 = now();
	errno = 0;
	if (b->fn() != 0) {
		fail(b->name, "failed");
		return 1;
	}
	t1 = now();
	printf("%s\t%s\t%.6f\tlower\n", b->group, b->name, t1 - t0);
	fflush(stdout);
	return 0;
}

/* -x: run a command and print its wall time. The status is the command's. */
static int
timed(const char *group, const char *name, char **argv)
{
	double t0, t1;
	pid_t pid;
	int status;

	t0 = now();
	if ((pid = fork()) == -1) {
		fail(name, "fork");
		return 1;
	}
	if (pid == 0) {
		execvp(argv[0], argv);
		_exit(127);
	}
	if (waitpid(pid, &status, 0) == -1) {
		fail(name, "waitpid");
		return 1;
	}
	t1 = now();
	if (!WIFEXITED(status) || WEXITSTATUS(status) != 0) {
		fprintf(stderr, "rb-bench: %s: %s exited with %d\n", name, argv[0], status);
		return 1;
	}
	printf("%s\t%s\t%.6f\tlower\n", group, name, t1 - t0);
	return 0;
}

static const char *words[] = {
	"alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf", "hotel",
	"india", "juliett", "kilo", "lima", "mike", "november", "oscar", "papa",
};

/* -g: write a fixed input of the tools benchmarks. */
static int
generate(const char *kind, const char *path, uint64_t n)
{
	FILE *f;
	uint64_t i, x;
	char name[1024];

	rng_state = 0x9e3779b97f4a7c15ULL;
	if (strcmp(kind, "tree") == 0) {
		if (mkdir(path, 0755) == -1 && errno != EEXIST)
			return 1;
		for (i = 0; i < n; i++) {
			if (i % 100 == 0) {
				snprintf(name, sizeof(name), "%s/d%llu", path, (unsigned long long)(i / 100));
				if (mkdir(name, 0755) == -1 && errno != EEXIST)
					return 1;
			}
			snprintf(name, sizeof(name), "%s/d%llu/f%llu", path,
			    (unsigned long long)(i / 100), (unsigned long long)i);
			if ((f = fopen(name, "w")) == NULL)
				return 1;
			x = 512 + rng() % 7680;
			while (x > 0) {
				const char *w = words[rng() % 16];
				fputs(w, f);
				fputc(' ', f);
				x = x > strlen(w) + 1 ? x - strlen(w) - 1 : 0;
			}
			fclose(f);
		}
		return 0;
	}
	if ((f = fopen(path, "w")) == NULL)
		return 1;
	if (strcmp(kind, "log") == 0) {
		uint64_t bytes = n << 20, done = 0;
		int len;

		for (i = 0; done < bytes; i++) {
			x = rng();
			len = fprintf(f,
			    "2026-%02u-%02uT%02u:%02u:%02u host%03u %s[%u]: %s %s value=%u status=%s\n",
			    (unsigned)(x % 12 + 1), (unsigned)(x >> 8) % 28 + 1, (unsigned)(x >> 16) % 24,
			    (unsigned)(x >> 24) % 60, (unsigned)(x >> 32) % 60, (unsigned)(x >> 40) % 1000,
			    words[(x >> 50) % 16], (unsigned)(x >> 20) % 65536, words[(x >> 54) % 16],
			    words[(x >> 58) % 16], (unsigned)(x >> 12) % 100000,
			    (x >> 62) == 0 ? "error" : "ok");
			if (len < 0)
				return 1;
			done += (uint64_t)len;
		}
	} else if (strcmp(kind, "lines") == 0) {
		for (i = 0; i < n; i++) {
			x = rng();
			fprintf(f, "%s%llu %s\n", words[x % 16], (unsigned long long)(x >> 20),
			    words[(x >> 4) % 16]);
		}
	} else {
		fclose(f);
		fprintf(stderr, "rb-bench: no input kind %s\n", kind);
		return 2;
	}
	return fclose(f) == 0 ? 0 : 1;
}

static void
usage(void)
{
	fprintf(stderr,
	    "usage: rb-bench [-s SCALE] [NAME ...]\n"
	    "       rb-bench -l\n"
	    "       rb-bench -V\n"
	    "       rb-bench -x GROUP NAME CMD [ARG ...]\n"
	    "       rb-bench -g log|lines|tree PATH N\n");
	exit(2);
}

int
main(int argc, char **argv)
{
	size_t i;
	int a, rc = 0, found;

	if (argc > 1 && strcmp(argv[1], "-l") == 0) {
		for (i = 0; i < NBENCH; i++)
			printf("%s\t%s\n", benches[i].group, benches[i].name);
		return 0;
	}
	if (argc > 1 && strcmp(argv[1], "-V") == 0) {
		printf("rb-bench %s\n", VERSION);
		return 0;
	}
	if (argc > 1 && strcmp(argv[1], "-x") == 0) {
		if (argc < 5)
			usage();
		return timed(argv[2], argv[3], argv + 4);
	}
	if (argc > 1 && strcmp(argv[1], "-g") == 0) {
		if (argc != 5)
			usage();
		return generate(argv[2], argv[3], strtoull(argv[4], NULL, 10));
	}
	a = 1;
	if (argc > 2 && strcmp(argv[1], "-s") == 0) {
		scale = strtod(argv[2], NULL);
		if (!(scale > 0))
			usage();
		a = 3;
	}
	signal(SIGPIPE, SIG_IGN);
	for (i = 0; i < NBENCH; i++) {
		if (a < argc) {
			found = 0;
			for (int k = a; k < argc; k++)
				found |= strcmp(argv[k], benches[i].name) == 0;
			if (!found)
				continue;
		}
		rc |= run(&benches[i]);
	}
	return rc;
}
