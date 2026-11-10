#!/usr/bin/env python3
"""Write the compile.jsonl of rb from the shim log of an R TOOLDIR.

    shim-records.py LOG SIDE FROM TO > compile.jsonl

LOG is the shim.tsv that conf/netbsd/toolchain/shim-*.sh write: one line for each call, with the
exit status, the directory, the slot and the arguments, separated by tabs. SIDE is G or R. Each
FROM in a path or an argument becomes TO, so that the objects of two builds of the same tree can
be unpacked side by side and paired by rb with --g-root and --r-root.

A record has the fields that rb flags-diff, rb sections-diff and rb symbols-diff read: the
arguments, the directory, the exit status, the slot, the side, the source as the input and the
object as the output. The shims keep no time, no environment and no hashes, so those are empty.
"""

import json
import os
import sys

SOURCES = (".c", ".S", ".s")


def record(line, side, old, new):
    fields = line.rstrip("\n").split("\t")
    if len(fields) < 3:
        return None
    status, cwd, slot = fields[0], fields[1].replace(old, new), fields[2]
    args = [a.replace(old, new) for a in fields[3:]]
    inputs, outputs = [], []
    after_o = False
    for a in args:
        if after_o:
            outputs.append(a)
            after_o = False
        elif a == "-o":
            after_o = True
        elif a.endswith(SOURCES) and not a.startswith("-"):
            inputs.append(a)
    if "-c" in args and not outputs and inputs:
        stem = os.path.splitext(os.path.basename(inputs[0]))[0]
        outputs.append(stem + ".o")
    return {
        "started": 0.0,
        "argv": ["x86_64--netbsd-" + ("cpp" if slot == "cpp" else "gcc")] + args,
        "compiler": "",
        "cwd": cwd,
        "inputs": [{"path": p, "sha256": ""} for p in inputs],
        "outputs": [{"path": p, "sha256": ""} for p in outputs],
        "wall-seconds": 0.0,
        "exit": int(status) if status.lstrip("-").isdigit() else None,
        "slot": slot,
        "side": side,
    }


def main():
    if len(sys.argv) != 5 or sys.argv[2] not in ("G", "R"):
        sys.exit("usage: shim-records.py LOG G|R FROM TO")
    log, side, old, new = sys.argv[1:]
    with open(log, encoding="utf-8", errors="replace") as f:
        for line in f:
            r = record(line, side, old, new)
            if r is not None:
                print(json.dumps(r))


if __name__ == "__main__":
    main()
