#!/usr/bin/env python3
"""Compare the manifests of two NetBSD builds from sets-manifest.py (plan 16.1).

    sets-compare.py FIRST SECOND --out DIR

DIR gets compare.json, compare.md and manifest.diff. A member differs in content when its hash or
link differs, and in metadata when only its mode, owner, time or size differs. The exit status is 0
when every file is the same.
"""

import argparse
import difflib
import json
import os
import sys


def parse(path):
    files, members = {}, {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            w = line.rstrip("\n").split(" ")
            if w[0] == "file" and len(w) == 4:
                files[w[1]] = (w[2], w[3])
            elif w[0] == "member" and len(w) >= 10:
                name = " ".join(w[2:-7])
                members[(w[1], name)] = w[-7:]
    return files, members


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("first")
    ap.add_argument("second")
    ap.add_argument("--out", required=True)
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    fa, ma = parse(a.first)
    fb, mb = parse(a.second)
    files = sorted(set(fa) | set(fb))
    differ = [p for p in files if fa.get(p) != fb.get(p)]
    content, meta, mtime_only = [], [], []
    for k in sorted(set(ma) & set(mb)):
        x, y = ma[k], mb[k]
        if x == y:
            continue
        if x[-1] != y[-1]:
            content.append(k)
        elif [x[i] for i in (0, 1, 2, 3, 5)] == [y[i] for i in (0, 1, 2, 3, 5)]:
            mtime_only.append(k)
        else:
            meta.append(k)
    only_a = sorted(set(ma) - set(mb))
    only_b = sorted(set(mb) - set(ma))
    rec = {
        "files": len(files),
        "files_differ": differ,
        "members": len(ma),
        "content": [f"{s}:{n}" for s, n in content],
        "metadata": [f"{s}:{n}" for s, n in meta],
        "mtime_only": [f"{s}:{n}" for s, n in mtime_only],
        "only_first": [f"{s}:{n}" for s, n in only_a],
        "only_second": [f"{s}:{n}" for s, n in only_b],
    }
    with open(os.path.join(a.out, "compare.json"), "w", encoding="utf-8") as f:
        json.dump(rec, f, indent=1)
        f.write("\n")
    with open(a.first, encoding="utf-8") as f1, open(a.second, encoding="utf-8") as f2:
        diff = difflib.unified_diff(f1.read().splitlines(), f2.read().splitlines(), "first", "second", lineterm="", n=0)
        with open(os.path.join(a.out, "manifest.diff"), "w", encoding="utf-8") as f:
            f.writelines(line + "\n" for line in diff)
    md = ["### Two G builds of NetBSD", ""]
    md.append(f"{len(files)} files in the sets and kernel directories. {len(differ)} differ.")
    md.append("")
    md.append(
        f"{len(ma)} members in the sets of the first build. {len(content)} differ in content, "
        f"{len(meta)} in mode, owner or size, {len(mtime_only)} only in time. {len(only_a)} are only in the "
        f"first build and {len(only_b)} only in the second."
    )
    listed = [("file", p) for p in differ]
    listed += [("content", f"{s}:{n}") for s, n in content] + [("metadata", f"{s}:{n}") for s, n in meta]
    listed += [("time", f"{s}:{n}") for s, n in mtime_only]
    listed += [("first only", f"{s}:{n}") for s, n in only_a] + [("second only", f"{s}:{n}") for s, n in only_b]
    if listed:
        md += ["", "```"]
        md += [f"{k}\t{p}" for k, p in listed[:200]]
        if len(listed) > 200:
            md.append(f"... and {len(listed) - 200} more")
        md.append("```")
    with open(os.path.join(a.out, "compare.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(md) + "\n")
    print("\n".join(md))
    return 0 if not differ else 1


if __name__ == "__main__":
    sys.exit(main())
