#!/usr/bin/env python3
"""Compare the ATF counts of a NetBSD baseline with the published run (plan 16.1).

    netbsd-gap.py BASELINES PUBLISHED.counts

BASELINES is the --dir of rb baseline. PUBLISHED.counts is a file of results/netbsd/published in
the format of results/README.md. The counts are: cases, the ATF test cases that ended; and passed,
the cases that passed or ended with one of the expected_* results, as atf-report counts them. For
the baseline, "every run" counts the cases with the same result in every run. The gap is
(G - published) / published.
"""

import glob
import os
import sys

import tomllib


def gap(g, p):
    return f"{100.0 * (g - p) / p:+.2f}%" if p else "-"


def good(result):
    return result == "passed" or result.startswith("expected_")


def main():
    base, pub_file = sys.argv[1], sys.argv[2]
    tomls = glob.glob(os.path.join(base, "**", "baseline.toml"), recursive=True)
    if not tomls:
        print("No baseline.toml.")
        return 1
    with open(tomls[0], "rb") as f:
        b = tomllib.load(f)
    p = {}
    with open(pub_file, encoding="utf-8") as f:
        for line in f:
            if line.startswith("#") or "=" not in line:
                continue
            k, _, v = line.rstrip("\n").partition("=")
            p[k] = v
    p_cases = int(p.get("cases", 0))
    p_pass = int(p.get("passed", 0)) + int(p.get("expected_failures", 0))
    rows = []
    for r in b.get("run", []):
        t = r.get("counts", {}).get("tests", {})
        rows.append((f"run {r['n']}", sum(t.values()), sum(n for k, n in t.items() if good(k))))
    n = int(b.get("runs", 0))
    every = [0, 0]
    with open(os.path.join(os.path.dirname(tomls[0]), "cases.tsv"), encoding="utf-8") as f:
        for line in f:
            cols = line.rstrip("\n").split("\t")
            if line.startswith("#") or cols[0] != "tests":
                continue
            results = set(cols[2 : 2 + n])
            every[0] += 1
            if len(results) == 1 and good(results.pop()):
                every[1] += 1
    rows.append(("every run", every[0], every[1]))
    print()
    print("### ATF against the published run")
    print()
    print(
        f"Published: {p.get('release', '')} on {p.get('date', '')}, {p.get('host', '')}, {p_cases} cases, "
        f"{p.get('passed')} passed and {p.get('expected_failures')} expected failures, so {p_pass} passed."
    )
    print()
    print("| G | cases | gap | passed | gap |")
    print("| --- | --- | --- | --- | --- |")
    for name, cases, passed in rows:
        print(f"| {name} | {cases} | {gap(cases, p_cases)} | {passed} | {gap(passed, p_pass)} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
