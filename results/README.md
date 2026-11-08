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

## How the files are used

The published run is a cross-check, not the oracle (`docs/plan/12-boot-and-tests.md`, section 12.1). If our G counts are more than 2% away from it, the rig differs from the project's, and we fix the rig before we grade R. The oracle is G itself: a case counts only if G passed it in all three baseline runs.

The published files come from the `test.tps` of the NetBSD-current amd64 bare-metal run at `https://www.gson.org/netbsd/bugs/build/amd64-baremetal/`. They record the source URL in the `source` key.
