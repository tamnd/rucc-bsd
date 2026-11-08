#!/usr/bin/env python3
"""Count ATF results from atf-run output, and compare two sets of counts.

    atf-counts.py count --out RUN.counts [--cases RUN.cases.tsv] [--set KEY=VALUE]... TPS...
    atf-counts.py compare REFERENCE.counts OTHER.counts...

A .counts file is the format of results/README.md: "key=value" lines, keys in a fixed order.
The cases file has one line per test case: "program<TAB>case<TAB>result<TAB>seconds".

The result names are atf-run's. The expected_* results (failure, death, exit, signal and timeout)
together are the "expected failures" that atf-report prints. A test program that ends with a
reason on its tp-end line, for example because it crashed or timed out as a whole, is "broken";
its cases that never ended are not counted as cases.
"""

import argparse
import sys

RESULTS = ["passed", "failed", "skipped", "expected_failure", "expected_death", "expected_exit",
           "expected_signal", "expected_timeout"]
KEYS = ["programs", "broken_programs", "cases", "passed", "failed", "skipped",
        "expected_failures"] + RESULTS[3:]


def parse_tps(path, counts, cases):
    program = None
    started = {}
    with open(path, "rb") as f:
        for raw in f:
            line = raw.decode("latin-1").rstrip("\n")
            if line.startswith("tp-start: "):
                parts = line[len("tp-start: "):].split(", ")
                program = parts[1]
                counts["programs"] += 1
            elif line.startswith("tp-end: "):
                parts = line[len("tp-end: "):].split(", ", 2)
                if len(parts) > 2 and parts[2].strip():
                    counts["broken_programs"] += 1
                program = None
            elif line.startswith("tc-start: "):
                ts, name = line[len("tc-start: "):].split(", ", 1)
                started[name] = float(ts)
            elif line.startswith("tc-end: "):
                parts = line[len("tc-end: "):].split(", ", 3)
                ts, name, result = float(parts[0]), parts[1], parts[2]
                counts["cases"] += 1
                counts[result] = counts.get(result, 0) + 1
                seconds = ts - started.pop(name, ts)
                cases.append((program or "?", name, result, f"{seconds:.2f}"))


def write_counts(path, meta, counts):
    counts["expected_failures"] = sum(counts.get(r, 0) for r in RESULTS[3:])
    with open(path, "w") as f:
        for k, v in meta:
            f.write(f"{k}={v}\n")
        for k in KEYS:
            f.write(f"{k}={counts.get(k, 0)}\n")
        for k in sorted(counts):
            if k not in KEYS:
                f.write(f"{k}={counts[k]}\n")


def read_counts(path):
    d = {}
    with open(path) as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                d[k] = v
    return d


def cmd_count(args):
    counts = {k: 0 for k in KEYS}
    cases = []
    for p in args.tps:
        parse_tps(p, counts, cases)
    meta = [tuple(s.split("=", 1)) for s in args.set]
    write_counts(args.out, meta, counts)
    if args.cases:
        with open(args.cases, "w") as f:
            for c in sorted(cases):
                f.write("\t".join(c) + "\n")
    print(open(args.out).read(), end="")


def cmd_compare(args):
    ref = read_counts(args.reference)
    others = [read_counts(p) for p in args.other]
    label = lambda d, p: d.get("label") or p
    head = [label(ref, args.reference)] + [label(d, p) for d, p in zip(others, args.other)]
    print("| | " + " | ".join(head) + " |")
    print("|---" * (len(head) + 1) + "|")
    for k in ["cases", "passed", "failed", "skipped", "expected_failures", "broken_programs"]:
        r = int(ref.get(k, 0))
        cells = [str(r)]
        for d in others:
            v = int(d.get(k, 0))
            pct = f" ({(v - r) * 100 / r:+.1f}%)" if r else ""
            cells.append(f"{v}{pct}")
        print(f"| {k} | " + " | ".join(cells) + " |")


def main():
    ap = argparse.ArgumentParser()
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("count")
    c.add_argument("--out", required=True)
    c.add_argument("--cases")
    c.add_argument("--set", action="append", default=[])
    c.add_argument("tps", nargs="+")
    m = sub.add_parser("compare")
    m.add_argument("reference")
    m.add_argument("other", nargs="+")
    args = ap.parse_args()
    {"count": cmd_count, "compare": cmd_compare}[args.cmd](args)


if __name__ == "__main__":
    sys.exit(main())
