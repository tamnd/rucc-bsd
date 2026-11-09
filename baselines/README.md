# Baselines

A baseline is the reference result of one pin, row and build configuration (plan 12.5). G, the build with the reference GCC, is booted and tested several times, three by default, each time on a fresh copy of the same image. A test case is graded only if G passed it in every run. Every other case is `g-unstable` and is never graded (12.1).

`rb baseline` writes the files. Do not edit them by hand. Baselines are kept forever, so a result can always be traced to the oracle it was graded against. A baseline is made again when the pin moves, the reference toolchain moves, or the rig changes.

## How to make one

```
rb baseline freebsd-15.1 --row amd64 --conf graded --image G.img
```

This boots `G.img` three times with `rb boot` and the suites `boot,smoke,tests`, with the output of each boot in `work/baseline/freebsd/amd64/15.1/graded/run-N`. It takes the options of `rb boot`, such as `--firmware` and `--tests-only`. `--runs N` changes the number of runs.

A workflow that boots in separate jobs runs `rb boot --suite boot,smoke,tests` in each job, and then gives the output directories to `rb baseline` with `--from`:

```
rb baseline netbsd-trunk --row amd64 --conf graded --from run-1,run-2,run-3 --image G.img
```

A run can be in shards, one boot per shard, when the whole suite does not fit in one job. Join the directories of the shards of one run with `+`. This is one run in three shards:

```
rb baseline netbsd-trunk --row amd64 --conf graded --from net+lib+rest
```

A case that more than one shard ran, such as `boot`, keeps its worse result. The run passed only if every shard passed.

`rb baseline` exits 1 when a run did not pass, for example after a panic or a timeout. It still writes the files, with `complete = false`.

A complete baseline is also the record that G is good at its `commit` (plan 04.6). `rb sets` reads `pin`, `commit`, `made` and `complete` from every `baseline.toml`. A branch pin is `reference = "good"` at the commit of the newest complete baseline of that pin, and under `head = "last-g-good"` it stays at that commit until a complete baseline names the head. So nobody edits `pins.toml` by hand to move a pin.

## Layout

```
baselines/
  <system>/<row>/<version>/<conf>/
    baseline.toml      what was run, the image hash, and per run its outcome and counts
    cases.tsv          per case, the result and the duration of each run, and the verdict
    stable-pass.txt    the cases that passed in every run
    kernel-log.txt     the kernel messages of the runs, as the allowed set
```

`<version>` is the pin name without the system, for example `15.1` for the pin `freebsd-15.1` and `trunk` for `netbsd-trunk`. `<conf>` is the build configuration, for example `graded`.

## The case names

Each case belongs to a suite of 12.2.

| Suite | Case | Where the result comes from |
|---|---|---|
| `boot` | `boot` | the result of `rb boot`: `passed`, or what stopped it, such as `panic` or `timeout` |
| `smoke` | the check, such as `network` | `smoke/results`, with `pass`, `fail` and `skip` written as `passed`, `failed` and `skipped` |
| `tests` | `DIR/PROGRAM:CASE`, such as `bin/sh/builtins:cd1` | `tests/kyua.log` on FreeBSD, `tests/atf/DIR.tps` on NetBSD |

`DIR/PROGRAM` is the path of the test program under `/usr/tests`. The results keep the names that kyua and atf-run give them: `passed`, `failed`, `skipped`, `broken`, and the `expected_*` results. A NetBSD case that started and did not end, because its test program crashed, is `broken`.

## `baseline.toml`

| Key | Meaning |
|---|---|
| `pin`, `system`, `version`, `commit` | the tree. `version` and `commit` come from `pins.toml` and are empty for a pin that is not there |
| `row`, `conf` | the row of `rows.toml` and the build configuration |
| `made` | when the baseline was written, in UTC |
| `image`, `image_sha256` | the image's file name and SHA-256, so that a rebuild of G can be checked to be byte-identical |
| `suites` | the suites the runs asked for |
| `runs` | the number of runs |
| `complete` | whether every run passed. Only a complete baseline is used to grade |
| `cases`, `stable_pass`, `g_unstable` | the cases seen in any run, those that passed in every run, and the rest |

Then one `[[run]]` table per run:

| Key | Meaning |
|---|---|
| `n` | the run, from 1 |
| `result`, `passed` | the result of `rb boot`, and whether the boot passed |
| `seconds` | how long the boot took in all |
| `firmware`, `accel`, `restarts` | `ovmf` or `seabios`, `kvm` or `tcg`, and how often the guest was started again after a reboot |
| `panic`, `panic_case` | the panic message and the test case that was running, after a panic |
| `cases` | the cases of this run |
| `counts.<suite>.<result>` | the cases of each suite with each result |

## `cases.tsv`

One line per case, sorted by suite and name, with tabs between the fields. The first line names the fields and starts with `#`.

```
# suite	case	run-1	run-2	run-3	seconds-1	seconds-2	seconds-3	verdict
tests	bin/sh/builtins:a	passed	passed	passed	0.010	0.020	0.030	stable-pass
tests	bin/sh/builtins:b	passed	failed	passed	0.100	0.100	0.100	g-unstable
```

A case that a run did not report is `missing` in that run. A duration the tool did not give is `-`. The verdict is `stable-pass` if the case passed in every run, and `g-unstable` otherwise.

## `stable-pass.txt`

The cases with the verdict `stable-pass`, one per line: the suite, a tab, and the case. The lines are sorted. This is the set that R is graded against.

## `kernel-log.txt`

The kernel messages of the runs, from `dmesg.end`, or from `dmesg.boot` where that is missing. The parts that change from boot to boot are replaced by `#`: the time stamp at the start of a NetBSD line, hex numbers, numbers with a decimal point, and numbers of three digits or more. Small numbers stay, because they name devices such as `vtnet0`. Each line is the number of runs that printed the message, a tab, and the message. A message in R's console that is not in this file is a finding, even if every test passed (12.1).
