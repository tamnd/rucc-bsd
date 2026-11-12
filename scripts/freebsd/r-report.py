#!/usr/bin/env python3
"""The units of an R build that failed, grouped by cause, from the build's compile.jsonl.

    r-report.py LOG [--src DIR] [--tsv FILE] [--limit N]

LOG is the compile.jsonl that rb-cc wrote, or the same file compressed with xz. A unit is a call
of the C compiler slot that is not a probe and has a C or assembly input. It failed when it exited
with a status other than 0 or was killed by a signal.

The cause of a failure is the first error line of its stderr: an "error:" line, a "fatal error:"
line, a Rust panic or a signal. The file, line and column at the front are removed, and quoted
names and numbers are replaced, so that one cause in many files makes one group. Each group lists
the names it replaced, most frequent first, and some of its units.

The report is Markdown on standard output. --tsv writes one line per failed unit: the group
number, the unit and the first error line as rucc wrote it. --src is the source tree, whose
prefix is removed from the unit names; /build/src by default.

A unit that the shim delegated under bring-up (plan 00) is listed after the groups, with its
class and its issue, and the first line says that the build is ungraded.
"""

import argparse
import collections
import json
import lzma
import re
import sys

SOURCES = (".c", ".S", ".s", ".i")
LOCATION = re.compile(r"^(?:\S+?:\d+(?::\d+)?:\s*)+")
QUOTED = re.compile(r"'[^']*'|`[^`']*'|\"[^\"]*\"|‘[^’]*’")
NUMBER = re.compile(r"\b0x[0-9a-fA-F]+\b|\b\d+\b")
PANIC = re.compile(r"panicked at ([^:\s]+):\d+:\d+:?\s*(.*)")


def records(path):
    opener = lzma.open if path.endswith(".xz") else open
    with opener(path, "rt", encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            try:
                yield json.loads(line)
            except json.JSONDecodeError:
                continue


def unit_of(rec, src):
    for i in rec.get("inputs") or []:
        p = i.get("path", "")
        if p.endswith(SOURCES):
            if p.startswith(src + "/"):
                p = p[len(src) + 1:]
            return p
    return None


def first_error(rec):
    """The first error line, as written, and the cause it gives."""
    lines = (rec.get("stderr") or "").splitlines()
    for n, line in enumerate(lines):
        m = PANIC.search(line)
        if m:
            text = m.group(2) or (lines[n + 1] if n + 1 < len(lines) else "")
            return line.strip(), f"panic at {m.group(1)}: {normal(text)[0]}", normal(text)[1]
    for line in lines:
        if re.search(r"\b(fatal error|error|internal compiler error)\b\s*:", line):
            key, names = normal(line)
            return line.strip(), key, names
    if rec.get("signal"):
        return f"signal {rec['signal']}", f"killed by signal {rec['signal']}", []
    tail = next((l for l in reversed(lines) if l.strip()), "")
    if tail:
        key, names = normal(tail)
        return tail.strip(), "no error line; last line: " + key, names
    return f"exit {rec.get('exit')}", f"exit {rec.get('exit')} with no output", []


def normal(line):
    """The line without its location, with quoted names and numbers replaced."""
    line = LOCATION.sub("", line.strip())
    names = [q[1:-1] for q in QUOTED.findall(line)]
    line = QUOTED.sub("'X'", line)
    line = NUMBER.sub("N", line)
    return re.sub(r"\s+", " ", line).strip(), names


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("log")
    ap.add_argument("--src", default="/build/src")
    ap.add_argument("--tsv")
    ap.add_argument("--limit", type=int, default=40)
    a = ap.parse_args()

    units = ok = probes = 0
    by_origin = collections.Counter()
    groups = collections.OrderedDict()
    failed = []
    delegated = {}
    for rec in records(a.log):
        if rec.get("slot") not in (None, "cc"):
            continue
        if rec.get("probe"):
            probes += 1
            continue
        unit = unit_of(rec, a.src)
        if unit is None:
            continue
        units += 1
        by_origin[rec.get("origin") or "none"] += 1
        if rec.get("delegated"):
            cls, _, issue = rec["delegated"].partition(" ")
            delegated[unit] = (cls, issue.strip() or "none")
        if rec.get("exit") == 0 and not rec.get("signal"):
            ok += 1
            continue
        text, key, names = first_error(rec)
        g = groups.setdefault(key, {"units": [], "names": collections.Counter()})
        g["units"].append(unit)
        g["names"].update(names)
        failed.append((key, unit, text))

    ordered = sorted(groups.items(), key=lambda kv: -len(kv[1]["units"]))
    number = {key: n + 1 for n, (key, _) in enumerate(ordered)}
    out = sys.stdout
    if delegated:
        out.write(f"Ungraded. Units delegated to the reference under bring-up: {len(delegated)}.\n\n")
    out.write(f"{units} units of the C compiler slot: {ok} compiled, {units - ok} failed, in {len(groups)} groups. {probes} probes are not counted.\n\n")
    if by_origin:
        out.write("Origins: " + ", ".join(f"{k} {v}" for k, v in sorted(by_origin.items())) + ".\n\n")
    if ordered:
        out.write("| # | units | cause | names | some units |\n|---:|---:|---|---|---|\n")
        for key, g in ordered[: a.limit]:
            names = ", ".join(f"`{n}` {c}" for n, c in g["names"].most_common(4))
            some = ", ".join(f"`{u}`" for u in sorted(set(g["units"]))[:3])
            cause = key.replace("|", "\\|")
            out.write(f"| {number[key]} | {len(g['units'])} | {cause} | {names} | {some} |\n")
        if len(ordered) > a.limit:
            rest = sum(len(g["units"]) for _, g in ordered[a.limit:])
            out.write(f"\n{len(ordered) - a.limit} more groups with {rest} units are in the TSV file.\n")
    if delegated:
        out.write("\n| delegated unit | class | issue |\n|---|---|---|\n")
        for unit, (cls, issue) in sorted(delegated.items())[: a.limit]:
            out.write(f"| `{unit}` | {cls} | {issue} |\n")
    if a.tsv:
        with open(a.tsv, "w", encoding="utf-8") as f:
            f.write("group\tunit\tfirst error\n")
            for key, unit, text in sorted(failed, key=lambda t: (number[t[0]], t[1])):
                f.write(f"{number[key]}\t{unit}\t{text.replace(chr(9), ' ')}\n")


if __name__ == "__main__":
    main()
