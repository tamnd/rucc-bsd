#!/bin/sh
# The run-time compile tests of B10 (plan 16.11), run as root in a booted RR image after pkg add
# of the rucc package. They are reported and not graded, because the G image has no C compiler.
#
#   cc.sh OUTDIR
#
# Each check compiles and runs a small program with cc, which is the rucc of the package, and
# writes one line "pass NAME" or "fail NAME" to OUTDIR/results and its output to OUTDIR/NAME.log.
# OUTDIR/counts gets "PASSED/TOTAL". Then kyua runs the test programs of /usr/tests that compile at
# run time and that GG and RR skip for want of cc: lib/atf/libatf-c and usr.bin/lorder. The kyua
# report is in OUTDIR/kyua.
out=$1
mkdir -p "$out"
out=$(cd "$out" && pwd)
: > "$out/results"
work=$(mktemp -d /tmp/cc.XXXXXX)
cd "$work" || exit 1
PATH=/sbin:/bin:/usr/sbin:/usr/bin:/usr/local/sbin:/usr/local/bin
export PATH

check()
{
	local name="$1"
	shift
	if "$@" > "$out/$name.log" 2>&1; then
		echo "pass $name" >> "$out/results"
	else
		echo "fail $name" >> "$out/results"
	fi
}

cat > hello.c <<'EOF'
#include <stdio.h>
int main(void) { printf("hello from %s\n", __VERSION__); return 0; }
EOF
cat > math.c <<'EOF'
#include <math.h>
#include <stdio.h>
#include <stdlib.h>
int main(int argc, char **argv)
{
	double x = argc > 1 ? atof(argv[1]) : 2.0;
	long double l = (long double)x * x;
	printf("%.6f %.6f %.6Lf\n", sqrt(x), pow(x, 10.0), l);
	return fabs(sqrt(x) * sqrt(x) - x) < 1e-12 && pow(x, 10.0) == 1024.0 && l == 4.0L ? 0 : 1;
}
EOF
cat > lib.c <<'EOF'
__thread int counter = 40;
int add(int a, int b) { return a + b + counter; }
EOF
cat > use.c <<'EOF'
#include <stdio.h>
extern __thread int counter;
int add(int, int);
int main(void) { counter++; int r = add(1, 0); printf("%d\n", r); return r == 42 ? 0 : 1; }
EOF
cat > threads.c <<'EOF'
#include <pthread.h>
#include <stdatomic.h>
#include <stdio.h>
static atomic_long total;
static _Thread_local long mine;
static void *work(void *arg)
{
	for (long i = 0; i < 100000; i++) {
		mine++;
		atomic_fetch_add_explicit(&total, 1, memory_order_relaxed);
	}
	return arg;
}
int main(void)
{
	pthread_t t[8];
	for (int i = 0; i < 8; i++)
		pthread_create(&t[i], NULL, work, NULL);
	for (int i = 0; i < 8; i++)
		pthread_join(t[i], NULL);
	printf("%ld %ld\n", (long)atomic_load(&total), mine);
	return atomic_load(&total) == 800000 && mine == 0 ? 0 : 1;
}
EOF
cat > qsort.c <<'EOF'
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
struct big { char name[24]; long v; double d; };
static int cmp(const void *a, const void *b)
{
	const struct big *x = a, *y = b;
	return (x->v > y->v) - (x->v < y->v);
}
static struct big pass(struct big b) { b.v *= 2; b.d /= 2; return b; }
int main(void)
{
	struct big a[1000];
	for (int i = 0; i < 1000; i++) {
		a[i].v = (i * 7919) % 1000;
		a[i].d = i;
		snprintf(a[i].name, sizeof a[i].name, "n%d", i);
	}
	qsort(a, 1000, sizeof a[0], cmp);
	for (int i = 0; i < 1000; i++)
		if (a[i].v != i)
			return 1;
	struct big b = pass(a[21]);
	return b.v == 42 && strcmp(b.name, a[21].name) == 0 ? 0 : 1;
}
EOF
mkdir -p prog
cat > prog/Makefile <<'EOF'
PROG=	hello
SRCS=	hello.c
MAN=
WARNS?=	6
.include <bsd.prog.mk>
EOF
cp hello.c prog/

check version sh -c 'command -v cc && cc --version && cc -v && rucc -dumpmachine'
check hello sh -c 'cc -o hello hello.c && ./hello'
# shellcheck disable=SC2016 # sh -c expands $o.
check optimize sh -c 'for o in -O0 -O1 -O2 -Os; do cc $o -o math$o math.c -lm && ./math$o || exit 1; done'
check static sh -c 'cc -static -O2 -o hello-static hello.c && ./hello-static && file hello-static | grep -q "statically linked"'
check pie sh -c 'cc -fPIE -pie -o hello-pie hello.c && ./hello-pie && file hello-pie | grep -q "pie executable\|shared object"'
check shared sh -c 'cc -fPIC -shared -o libadd.so lib.c && cc -o use use.c -L. -ladd -Wl,-rpath,'"$work"' && ./use'
check archive sh -c 'cc -c lib.c && ar rcs libadd.a lib.o && cc -o use-static use.c libadd.a && ./use-static'
check threads sh -c 'cc -O2 -pthread -o threads threads.c && ./threads'
check structs sh -c 'cc -O2 -o qsort qsort.c && ./qsort'
check debug sh -c 'cc -g -O0 -o hello-g hello.c && ./hello-g && readelf -S hello-g | grep -q debug_info'
check preprocess sh -c 'echo "__FreeBSD__ __x86_64__ __GNUC__" | cc -E - | tail -1'
check bsd_prog_mk sh -c 'make -C prog CC=cc && ./prog/hello'

passed=$(grep -c '^pass' "$out/results")
total=$(wc -l < "$out/results" | tr -d ' ')
echo "$passed/$total" > "$out/counts"
cd /
rm -rf "$work"

# The test programs that compile at run time.
mkdir -p "$out/kyua"
db=$out/kyua/kyua.db
(cd /usr/tests && timeout --foreground -s TERM -k 60 1800 kyua test -k /usr/tests/Kyuafile --results-file="$db" \
	lib/atf/libatf-c usr.bin/lorder > "$out/kyua/test.log" 2>&1)
kyua report --results-file="$db" --verbose --results-filter passed,skipped,xfail,broken,failed \
	--output "$out/kyua/report-verbose.txt"
kyua report --results-file="$db" --results-filter passed,skipped,xfail,broken,failed --output "$out/kyua/report.txt"
sed -n 's/^Test cases: //p' "$out/kyua/report.txt" | tr ' ' '_' > "$out/kyua/counts"
echo "cc checks $(cat "$out/counts"), kyua $(cat "$out/kyua/counts")"
