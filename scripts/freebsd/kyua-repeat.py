#!/usr/bin/env python3
"""The results of each case over the rounds of a kyua loop (freebsd-loop.yml), for G and for R.

    kyua-repeat.py --side G DIR... --side R DIR...

Each DIR is the OUTDIR of one boot.sh with RB_KYUA_REPEAT. Round 1 is results/out/kyua/test.log,
and round N is results/out/kyua/rounds/N.log. A line of kyua test is "PROGRAM:CASE  ->  RESULT ...".
The table has, for each case, how many rounds gave each result on each side.
"""

import argparse
import glob
import os
import re
import sys
from collections import Counter, defaultdict

LINE = re.compile(r"^(\S+:\S+)\s+->\s+(\w+)")


def logs(d):
    k = os.path.join(d, "results", "out", "kyua")
    found = []
    if os.path.isfile(os.path.join(k, "test.log")):
        found.append(os.path.join(k, "test.log"))
    found += sorted(glob.glob(os.path.join(k, "rounds", "*.log")),
                    key=lambda p: int(os.path.basename(p).split(".")[0]))
    return found


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--side", nargs="+", action="append", default=[], metavar=("SIDE", "DIR"))
    a = ap.parse_args()
    counts = defaultdict(lambda: defaultdict(Counter))
    rounds = Counter()
    sides = []
    for side, *dirs in a.side:
        sides.append(side)
        for d in dirs:
            for log in logs(d):
                rounds[side] += 1
                with open(log, encoding="utf-8", errors="replace") as f:
                    for line in f:
                        m = LINE.match(line.strip())
                        if m:
                            counts[m.group(1)][side][m.group(2)] += 1
    print("| case | " + " | ".join(f"{s} ({rounds[s]} rounds)" for s in sides) + " |")
    print("|---|" + "---|" * len(sides))
    for case in sorted(counts):
        cells = []
        for s in sides:
            c = counts[case][s]
            cells.append(", ".join(f"{r} {n}" for r, n in sorted(c.items())) or "not run")
        print(f"| {case} | " + " | ".join(cells) + " |")
    if not counts:
        print("No kyua results were found.")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
