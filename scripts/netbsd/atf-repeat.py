#!/usr/bin/env python3
"""The results of ATF cases that ran many times, by kernel, as a markdown table.

    atf-repeat.py --side G DIR... --side R DIR...

Each DIR is the output directory of one rb boot. The .tps files under DIR/results/tests/atf, also
in subdirectories, are read. rb boot --tests-repeat N writes run N of a directory as DIR@N.tps.
The table has one line for each case that passed at least once, and did not pass every time on
some side, with how many times it passed of how many runs and the median seconds on each side.
netbsd-loop.yml uses it.
"""

import argparse
import statistics
import sys
from pathlib import Path


def cases(path):
    """(case, result, seconds) for each case that ended in one .tps file."""
    out = []
    program, start = "?", {}
    for line in path.read_text(errors="replace").splitlines():
        if line.startswith("tp-start: "):
            program = line.split(", ")[1]
            start = {}
        elif line.startswith("tc-start: "):
            ts, _, case = line[len("tc-start: ") :].partition(", ")
            start[case] = float(ts)
        elif line.startswith("tc-end: "):
            parts = line[len("tc-end: ") :].split(", ", 3)
            if len(parts) < 3:
                continue
            seconds = None
            if parts[1] in start:
                seconds = float(parts[0]) - start.pop(parts[1])
            out.append((f"{program}:{parts[1]}", parts[2], seconds))
    return out


def directory(atf, path):
    """The test directory of a .tps file: its path under atf, with no .tps and no @N."""
    rel = str(path.relative_to(atf))[: -len(".tps")]
    base, at, n = rel.rpartition("@")
    return base if at and n.isdigit() else rel


def side(dirs):
    """case -> list of (result, seconds), over every run in dirs."""
    out = {}
    for d in dirs:
        atf = Path(d) / "results" / "tests" / "atf"
        for path in sorted(atf.rglob("*.tps")):
            top = directory(atf, path)
            for case, result, seconds in cases(path):
                # The .tps of one test program, such as lib/libc/sys/t_aio_lio, already ends
                # with the program's name.
                name = f"{top}/{case}"
                if case.split(":")[0] == top.rpartition("/")[2]:
                    name = f"{top}:{case.partition(':')[2]}"
                out.setdefault(name, []).append((result, seconds))
    return out


def cell(runs):
    if not runs:
        return "-", "-"
    passed = sum(1 for r, _ in runs if r == "passed")
    secs = [s for _, s in runs if s is not None]
    median = f"{statistics.median(secs):.1f}" if secs else "-"
    return f"{passed}/{len(runs)}", median


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--side",
        nargs="+",
        action="append",
        required=True,
        metavar=("NAME", "DIR"),
        help="a side's name and its rb boot directories",
    )
    args = ap.parse_args()
    sides = [(s[0], side(s[1:])) for s in args.side]
    names = sorted(set().union(*(r.keys() for _, r in sides)))
    print("| side | boots | cases | runs | passed |")
    print("|---|---|---|---|---|")
    for (name, runs), spec in zip(sides, args.side):
        total = sum(len(v) for v in runs.values())
        passed = sum(1 for v in runs.values() for r, _ in v if r == "passed")
        print(f"| {name} | {len(spec) - 1} | {len(runs)} | {total} | {passed} |")
    print()
    rows = []
    for case in names:
        per = [r.get(case, []) for _, r in sides]
        if all(runs and all(r == "passed" for r, _ in runs) for runs in per):
            continue
        if not any(r == "passed" for runs in per for r, _ in runs):
            continue
        rows.append([case] + [x for runs in per for x in cell(runs)])
    if not rows:
        print("No case passed on one run and did not pass on another.")
        return 0
    head = ["case"] + [f"{n} {x}" for n, _ in sides for x in ("passed", "median s")]
    print("| " + " | ".join(head) + " |")
    print("|---" * len(head) + "|")
    for row in rows:
        print("| " + " | ".join(row) + " |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
