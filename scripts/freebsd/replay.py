#!/usr/bin/env python3
"""Shell scripts that compile every unit of a G build again with another shim.

    replay.py LOG --shim PATH --out DIR [--shards N] [--objects DIR]

LOG is the compile.jsonl of a build through rb-cc-ref, or the same file compressed with xz. A
unit is a call of the C compiler slot that is not a probe, has a C or assembly input and has -c.
Each unit becomes one line: in the recorded directory, run PATH in place of the recorded shim,
with the recorded arguments. The object goes to the same path under DIR/objects (or --objects),
so that /build/obj/x/y.o becomes DIR/objects/build/obj/x/y.o, and the dependency file goes next
to it with .d in place of .o. So the G objects stay as they are, and rb sections-diff can pair
each object with its G object when the roots are given.

The units go round-robin into DIR/replay-0.sh to DIR/replay-(N-1).sh, 4 by default, to run at
the same time. Each line ends with "|| true", so one failed unit does not stop the script. The
shim writes the records; the scripts print nothing of their own.
"""

import argparse
import json
import lzma
import os
import shlex

SOURCES = (".c", ".S", ".s", ".i")


def records(path):
    opener = lzma.open if path.endswith(".xz") else open
    with opener(path, "rt", encoding="utf-8", errors="replace") as f:
        for line in f:
            line = line.strip()
            if line:
                try:
                    yield json.loads(line)
                except json.JSONDecodeError:
                    continue


def redirect(args, obj, dep):
    """The arguments with the object and the dependency file sent to obj and dep."""
    out, i, has_o = [], 0, False
    while i < len(args):
        a = args[i]
        if a == "-o" and i + 1 < len(args):
            out += ["-o", obj]
            has_o = True
            i += 2
            continue
        if a.startswith("-o") and len(a) > 2:
            out.append("-o" + obj)
            has_o = True
        elif a in ("-MF", "-MT", "-MQ") and i + 1 < len(args):
            out += [a, dep if a == "-MF" else args[i + 1]]
            i += 2
            continue
        elif a.startswith("-MF"):
            out.append("-MF" + dep)
        else:
            out.append(a)
        i += 1
    if not has_o:
        out += ["-o", obj]
    return out


def original(args, cwd, inputs):
    """The absolute path of the object the recorded call wrote."""
    obj = None
    for i, a in enumerate(args):
        if a == "-o" and i + 1 < len(args):
            obj = args[i + 1]
        elif a.startswith("-o") and len(a) > 2:
            obj = a[2:]
    if obj is None:
        source = next(i["path"] for i in inputs if i.get("path", "").endswith(SOURCES))
        obj = os.path.splitext(os.path.basename(source))[0] + ".o"
    return os.path.normpath(os.path.join(cwd, obj))


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("log")
    ap.add_argument("--shim", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--shards", type=int, default=4)
    ap.add_argument("--objects")
    a = ap.parse_args()
    objects = a.objects or os.path.join(a.out, "objects")
    os.makedirs(a.out, exist_ok=True)

    lines = []
    for rec in records(a.log):
        if rec.get("slot") not in (None, "cc") or rec.get("probe"):
            continue
        argv = rec.get("argv") or []
        if "-c" not in argv[1:]:
            continue
        inputs = rec.get("inputs") or []
        if not any(i.get("path", "").endswith(SOURCES) for i in inputs):
            continue
        cwd = rec.get("cwd", ".")
        obj = objects + original(argv[1:], cwd, inputs)
        args = redirect(argv[1:], obj, os.path.splitext(obj)[0] + ".d")
        cmd = " ".join(shlex.quote(x) for x in [a.shim, *args])
        mkdir = f"mkdir -p {shlex.quote(os.path.dirname(obj))}"
        lines.append(f"{mkdir} && cd {shlex.quote(cwd)} && {cmd} > /dev/null 2>&1 || true")

    for k in range(a.shards):
        with open(os.path.join(a.out, f"replay-{k}.sh"), "w", encoding="utf-8") as f:
            f.write("#!/bin/sh\n")
            f.writelines(line + "\n" for line in lines[k :: a.shards])
    print(f"{len(lines)} units in {a.shards} scripts in {a.out}")


if __name__ == "__main__":
    main()
