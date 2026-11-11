#!/usr/bin/env python3
"""Say what the rucc replay of a NetBSD world gave, as markdown, with the failed units by cause.

    replay-report.py UNITS SHIM.tsv TIMEOUTS FAILED.tsv

UNITS is the number of units that replay.py wrote. SHIM.tsv is the shim log of the replay: one
line for each call, with the exit status, the directory, the slot and the arguments. TIMEOUTS has
one line for each unit that timeout(1) stopped. Each failed unit has its standard error in the
.err file next to its object (replay.py --stderr).

The cause of a failed unit is its first error line, with the place, the quoted names and the
numbers taken out, so that one cause groups the units that fail in the same way. A unit with no
error line gets the signal or the exit status as its cause. FAILED.tsv gets one line for each
failed unit: the cause, the source, the directory, the first error line and the .err file.
"""

import collections
import os
import re
import sys

SOURCES = (".c", ".S", ".s")
ERROR = re.compile(r"\b(?:fatal )?error(?:\[(E\d+)\])?: (.*)")
CODE = re.compile(r"\s*\[(E\d+)\]\s*$")
QUOTED = re.compile(r"'[^']*'|\"[^\"]*\"|`[^`]*`")
NUMBER = re.compile(r"\b\d+\b")


def first_error(path):
    """The first error line of the .err file, or a line that says how the unit stopped."""
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            text = f.read()
    except OSError:
        return None, "no standard error file"
    panic = None
    lines = text.splitlines()
    for i, line in enumerate(lines):
        m = ERROR.search(line)
        if m:
            return line.strip(), None
        if panic is None and "panicked at" in line:
            # The message of a Rust panic is on the line after the place.
            panic = " ".join(x.strip() for x in lines[i : i + 2])
    return None, panic


def cause(line, other, status):
    if line is not None:
        m = ERROR.search(line)
        msg = m.group(2)
        code = m.group(1)
        c = CODE.search(msg)
        if c:
            code, msg = c.group(1), msg[: c.start()]
        msg = NUMBER.sub("N", QUOTED.sub("'_'", msg)).strip()
        return f"{code}: {msg}" if code else msg
    if other is not None and "panicked at" in other:
        return "rucc panicked: " + NUMBER.sub("N", other.split("panicked at", 1)[1]).strip()
    if status > 128:
        return f"signal {status - 128}"
    return f"exit {status} with no error line"


def main():
    if len(sys.argv) != 5:
        sys.exit("usage: replay-report.py UNITS SHIM.tsv TIMEOUTS FAILED.tsv")
    units, shim, timeouts, failed_out = int(sys.argv[1]), *sys.argv[2:]
    ran = 0
    failed = []
    if os.path.exists(shim):
        with open(shim, encoding="utf-8", errors="replace") as f:
            for raw in f:
                fields = raw.rstrip("\n").split("\t")
                if len(fields) < 3 or fields[2] != "cc":
                    continue
                ran += 1
                status = int(fields[0]) if fields[0].lstrip("-").isdigit() else -1
                if status == 0:
                    continue
                args = fields[3:]
                obj = next((args[i + 1] for i, a in enumerate(args[:-1]) if a == "-o"), "")
                source = next((a for a in args if a.endswith(SOURCES) and not a.startswith("-")), "")
                err = os.path.join(fields[1], os.path.splitext(obj)[0] + ".err")
                line, other = first_error(err)
                failed.append((cause(line, other, status), source, fields[1], line or other or "", err))
    stopped = []
    if os.path.exists(timeouts):
        with open(timeouts, encoding="utf-8", errors="replace") as f:
            stopped = [line.strip() for line in f if line.strip()]

    with open(failed_out, "w", encoding="utf-8") as f:
        for row in sorted(failed):
            f.write("\t".join(x.replace("\t", " ") for x in row) + "\n")

    sources = len({(r[2], r[1]) for r in failed})
    print("### The rucc replay of the world")
    print()
    print("| Units | Ran | Failed | Failed sources | Stopped |")
    print("|---|---|---|---|---|")
    print(f"| {units} | {ran} | {len(failed)} | {sources} | {len(stopped)} |")
    print()
    if failed:
        by = collections.defaultdict(list)
        for row in failed:
            by[row[0]].append(row)
        print("#### Failed units by cause")
        print()
        print("| Units | Cause | First unit | Its error |")
        print("|---|---|---|---|")
        for c, rows in sorted(by.items(), key=lambda kv: (-len(kv[1]), kv[0]))[:60]:
            first = rows[0]
            where = os.path.normpath(os.path.join(first[2], first[1]))
            print(f"| {len(rows)} | {md(c)} | `{where}` | {md(first[3][:200])} |")
        if len(by) > 60:
            print()
            print(f"{len(by) - 60} more causes are in {os.path.basename(failed_out)}.")
        print()
    if stopped:
        print("#### Units stopped by the timeout")
        print()
        for line in stopped[:20]:
            print(f"- `{line}`")
        print()


def md(text):
    return text.replace("|", "\\|").replace("\n", " ")


if __name__ == "__main__":
    main()
