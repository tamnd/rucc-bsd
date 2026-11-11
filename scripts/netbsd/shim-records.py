#!/usr/bin/env python3
"""Write the compile.jsonl of rb from the shim log of an R TOOLDIR.

    shim-records.py LOG SIDE FROM TO [ERRDIR] > compile.jsonl

LOG is the shim.tsv that conf/netbsd/toolchain/shim-*.sh write: one line for each call, with the
exit status, the directory, the slot and the arguments, separated by tabs. SIDE is G or R. Each
FROM in a path or an argument becomes TO, so that the objects of two builds of the same tree can
be unpacked side by side and paired by rb with --g-root and --r-root.

A record has the fields that rb flags-diff, rb sections-diff and rb symbols-diff read: the
arguments, the directory, the exit status, the slot, the side, the source as the input and the
object as the output. The shims keep no time, no environment and no hashes, so those are empty.

ERRDIR is the LOG.err that shim-ref.sh writes: the standard error of each call that wrote any,
in a file named by the first 16 hex digits of the SHA-256 of its log line. With ERRDIR, a record
has the first KiB of it as its stderr, as a record of rb-cc has, so that rb syntax can compare
the diagnostics of rucc with those of G.
"""

import hashlib
import json
import os
import sys

SOURCES = (".c", ".S", ".s")


def stderr(errdir, raw):
    """The first KiB of the standard error of the call of the log line RAW, in bytes, or nothing."""
    if not errdir:
        return ""
    name = hashlib.sha256(raw.rstrip(b"\n")).hexdigest()[:16]
    try:
        with open(os.path.join(errdir, name), "rb") as f:
            return f.read(1024).decode("utf-8", "replace")
    except OSError:
        return ""


def record(line, side, old, new, said=""):
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
    out = {
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
    if said:
        out["stderr"] = said
    return out


def main():
    if len(sys.argv) not in (5, 6) or sys.argv[2] not in ("G", "R"):
        sys.exit("usage: shim-records.py LOG G|R FROM TO [ERRDIR]")
    log, side, old, new = sys.argv[1:5]
    errdir = sys.argv[5] if len(sys.argv) == 6 else None
    with open(log, "rb") as f:
        for raw in f:
            line = raw.decode("utf-8", "replace")
            r = record(line, side, old, new, stderr(errdir, raw))
            if r is not None:
                print(json.dumps(r))


if __name__ == "__main__":
    main()
