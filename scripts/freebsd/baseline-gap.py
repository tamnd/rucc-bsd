#!/usr/bin/env python3
"""Compare the kyua counts of a baseline with the published run (plan 16.1).

    baseline-gap.py BASELINES PUBLISHED.json

BASELINES is the --dir of rb baseline. PUBLISHED.json is the output of published.py. The published
counts come from JUnit, where a failed and a broken case are failures and an expected failure is a
pass. So the counts here are: cases, the number of test cases kyua ran; and passed, the cases that
passed or failed as expected. For the baseline, passed is the cases that passed in every run plus
the cases that failed as expected in every run. The gap is (G - published) / published.
"""

import glob
import json
import os
import sys

import tomllib


def gap(g, p):
    return f"{100.0 * (g - p) / p:+.2f}%" if p else "-"


def main():
    base, pub_file = sys.argv[1], sys.argv[2]
    tomls = glob.glob(os.path.join(base, "**", "baseline.toml"), recursive=True)
    if not tomls:
        print("No baseline.toml.")
        return 1
    with open(tomls[0], "rb") as f:
        b = tomllib.load(f)
    try:
        with open(pub_file, encoding="utf-8") as f:
            p = json.load(f)
    except (OSError, ValueError):
        p = {}
    p_cases = p.get("total", 0)
    p_pass = p_cases - p.get("failed", 0) - p.get("skipped", 0) if p else 0
    rows = []
    for r in b.get("run", []):
        t = r.get("counts", {}).get("tests", {})
        cases = sum(t.values())
        passed = t.get("passed", 0) + t.get("expected_failure", 0)
        rows.append((f"run {r['n']}", cases, passed))
    # The cases with the same result in every run, from cases.tsv.
    stable = {"passed": 0, "expected_failure": 0, "any": 0}
    with open(os.path.join(os.path.dirname(tomls[0]), "cases.tsv"), encoding="utf-8") as f:
        for line in f:
            if line.startswith("#"):
                continue
            cols = line.rstrip("\n").split("\t")
            if cols[0] != "tests":
                continue
            results = set(cols[2:-1:2])
            stable["any"] += 1
            if len(results) == 1:
                r = results.pop()
                if r in stable:
                    stable[r] += 1
    rows.append(("every run", stable["any"], stable["passed"] + stable["expected_failure"]))
    print()
    print("### kyua against the published run")
    print()
    if p:
        print(
            f"Published: {p.get('job')} build {p.get('build')} on {p.get('commit', '')[:12]}, "
            f"{p_cases} cases, {p.get('failed')} failed or broken, {p.get('skipped')} skipped, so {p_pass} passed."
        )
    else:
        print("No published run.")
    print()
    print("| G | cases | gap | passed | gap |")
    print("| --- | --- | --- | --- | --- |")
    for name, cases, passed in rows:
        print(f"| {name} | {cases} | {gap(cases, p_cases)} | {passed} | {gap(passed, p_pass)} |")
    return 0


if __name__ == "__main__":
    sys.exit(main())
