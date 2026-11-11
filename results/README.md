# Results

This directory keeps the test results of the builds as small text files. Images, logs and per-case outputs stay in the workflow artifacts. They are too big for the history.

## Layout

```
results/
  netbsd/
    published/
      amd64-baremetal-<build>.counts    counts of a run that the NetBSD project published
      amd64-baremetal-<build>.failed    the cases that failed in that run
    <pin>-<commit>/amd64/
      g-<run>.counts                    counts of one of our runs of G
      g-<run>.failed                    the cases that failed in that run
```

Benchmark results (plan 13.2 and 13.3) have their own tree:

```
results/
  bench/<system>/<row>/<version>/
    bench.tsv                           the summary of one rb bench: R's speed against G per benchmark
    runs.tsv                            every value of every run
```

`<version>` is the pin name without the system, for example `15.1` for `freebsd-15.1`.

`<pin>` is the pin name in `conf/netbsd/trunk.pin`, for example `trunk`. `<commit>` is the short hash of the pinned tree. `<run>` is the GitHub Actions run ID. R runs use the prefix `r-` in the same place when they exist.

## The `.counts` format

One `key=value` pair per line. The first lines describe the run. The counts follow in a fixed order. Lines that start with `#` are comments.

| Key | Meaning |
|---|---|
| `label` | a short name for tables, `G`, `R` or `published` |
| `system`, `pin`, `commit`, `row` | the tree and the architecture |
| `cc`, `gcc` | the compiler of the build, and the in-tree GCC version |
| `kernel`, `image` | the kernel configuration and the kind of image that booted |
| `host`, `vm` | where QEMU ran, and the CPUs and memory of the guest |
| `run`, `date` | the workflow run and the day it ran |
| `dirs` | the top-level directories of `/usr/tests` that ran |
| `programs` | the test programs that started |
| `broken_programs` | the test programs that ended with a reason, for example a crash of the whole program |
| `cases` | the test cases that ended |
| `passed`, `failed`, `skipped` | the cases with each result |
| `expected_failures` | the sum of the five `expected_*` results, as `atf-report` counts them |
| `expected_failure`, `expected_death`, `expected_exit`, `expected_signal`, `expected_timeout` | the cases with each expected result |

`scripts/netbsd/atf-counts.py count` writes the file from the `atf-run` outputs (`.tps` files). `scripts/netbsd/atf-counts.py compare` prints two or more files as one table, with the difference from the first in percent.

## The `.failed` format

One line per failed case: the test program, a tab, and the case name. The lines are sorted.

## The benchmark files

`rb bench` writes both files. The bench suite of rb-rc (`guest/rb-rc/bench.sh`) runs the set once per boot with the `rb-bench` program (`bench/rb-bench.c`) that each side's own compiler built. `rb bench` boots R, then G, 10 times each by default.

`bench.tsv` starts with `#` lines, one `# key<TAB>value` each: `pin`, `system`, `row`, `made`, `g-image`, `r-image`, `rb-bench` (the version of the program), `runs`, `g-runs` and `r-runs` (the runs of each side that wrote results), and `geomean` when a benchmark is gated. Then a header line and one line per benchmark:

| Column | Meaning |
|---|---|
| `group`, `name` | the group of plan 13.2, such as `syscalls`, and the benchmark |
| `better` | `lower` for a time in seconds, `higher` for a rate |
| `g_median`, `r_median` | the median over the runs of each side |
| `g_cv`, `r_cv` | the coefficient of variation of each side, `-` with fewer than two values |
| `g_runs`, `r_runs` | the runs of each side with a value |
| `speed` | R's speed against G: G's time over R's, or R's rate over G's; under 1 is slower |
| `gated` | `yes`, or why not: fewer than 3 runs, a run with no value, or a CV on G above 1% |

`runs.tsv` has the columns `side` (`g` or `r`), `run`, `group`, `name`, `value` and `better`.

The headline is the geometric mean of the gated speeds. `rb speed` applies the gates of 02.5 to it.

## How the files are used

The published run is a cross-check, not the oracle (`docs/plan/12-boot-and-tests.md`, section 12.1). If our G counts are more than 2% away from it, the rig differs from the project's, and we fix the rig before we grade R. The oracle is G itself: a case counts only if G passed it in all three baseline runs.

The published files come from the `test.tps` of the NetBSD-current amd64 bare-metal run at `https://www.gson.org/netbsd/bugs/build/amd64-baremetal/`. They record the source URL in the `source` key.
