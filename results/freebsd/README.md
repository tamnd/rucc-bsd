# FreeBSD results

Each run of the freebsd-g workflow makes one summary. A summary that we keep is committed here as `<pin>-<conf>-<side>.json`, for example `15.1-release-graded-G.json`. The per-case results are too large to commit. They stay in the run's artifacts (`freebsd-results-<pin>-<conf>`, file `summary/cases.tsv`).

`scripts/freebsd/summarize.py` writes the summary from the build record and the boot directories. These are its keys:

- `system`, `row`, `conf`, `side`, `run`: what was built, and the GitHub Actions run ID.
- `source`: the pin, its kind (`txz` or `git`), the URL and the SHA-256 or the commit.
- `toolchain`: the version of each toolchain package that pkg installed in the builder VM.
- `phases`: one entry per phase (`world`, `kernel`, `install`, `overlay`, `image`). Each entry has the exit status, the seconds, the size in KB of what the phase made, and the KB in use on the build disk after it.
- `image`: the size of the raw image, the KB it allocates, the size and SHA-256 of the zstd file, and the size of the staging tree.
- `boots`: one entry per boot, named for the firmware (`bios`, `uefi`). Each entry has the result (`done`, `panic`, `no-done` or `no-ready`), the seconds to RB-READY, the seconds in all, and `kern.compiler_version`. The smoke results are under `smoke`. The kyua results are under `kyua`: the counts per result, the seconds, the exit status and the excluded tests.
- `published`: the counts of the last completed build of FreeBSD's own test job on ci.freebsd.org for the branch, with its build number, commit and URL.
- `gap`: our kyua counts minus the published counts. Jenkins counts a broken case as failed and an expected failure as passed, so `failed_and_broken` compares our failed plus broken with its failed.

`cases.tsv` has one line per test case: `program:case`, the result and the seconds.
