#!/usr/bin/env python3
"""Compare the records of two G builds of one pin (plan 16.1).

    repro.py FIRST SECOND --out DIR [--title TITLE]

FIRST and SECOND are build records of freebsd-g: image.txt, install.manifest and staging.manifest
from image.sh. The script compares the hashes of image.txt, and each manifest line by line. A
manifest has one stat line per file ("MODE OWNER GROUP SIZE PATH" or "... PATH -> TARGET") and one
hash line per regular file ("SHA256 PATH"). DIR gets repro.json, repro.md and one .diff file per
manifest with the lines that differ. The exit status is 0 when the images are the same.
"""

import argparse
import difflib
import json
import os
import re
import sys

HASHES = ["esp_sha256", "root_sha256", "raw_sha256", "zst_sha256"]
HASH_LINE = re.compile(r"^([0-9a-f]{64}) (\S.*)$")


def read(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return None


def image_txt(d):
    rec = {}
    for line in (read(os.path.join(d, "image.txt")) or "").splitlines():
        k, _, v = line.partition(" ")
        rec[k] = v.strip()
    return rec


def entries(text):
    """Map each path to its stat line and its hash."""
    out = {}
    for line in text.splitlines():
        m = HASH_LINE.match(line)
        if m:
            out.setdefault(m.group(2), {})["sha256"] = m.group(1)
            continue
        parts = line.split(" ", 4)
        if len(parts) == 5:
            path = parts[4].split(" -> ")[0]
            out.setdefault(path, {})["stat"] = line
    return out


def compare(name, a, b, out):
    ta, tb = read(os.path.join(a, name)), read(os.path.join(b, name))
    if ta is None or tb is None:
        return {"name": name, "missing": True}
    ea, eb = entries(ta), entries(tb)
    only_a = sorted(set(ea) - set(eb))
    only_b = sorted(set(eb) - set(ea))
    content = sorted(p for p in set(ea) & set(eb) if ea[p].get("sha256") != eb[p].get("sha256"))
    meta = sorted(
        p
        for p in set(ea) & set(eb)
        if ea[p].get("stat") != eb[p].get("stat") and ea[p].get("sha256") == eb[p].get("sha256")
    )
    diff = difflib.unified_diff(ta.splitlines(), tb.splitlines(), "first/" + name, "second/" + name, lineterm="", n=0)
    with open(os.path.join(out, name + ".diff"), "w", encoding="utf-8") as f:
        f.writelines(line + "\n" for line in diff)
    return {
        "name": name,
        "entries": len(ea),
        "only_first": only_a,
        "only_second": only_b,
        "content": content,
        "metadata": meta,
    }


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("first")
    ap.add_argument("second")
    ap.add_argument("--out", required=True)
    ap.add_argument("--title", default="Two G builds of one pin")
    a = ap.parse_args()
    os.makedirs(a.out, exist_ok=True)
    ia, ib = image_txt(a.first), image_txt(a.second)
    hashes = {k: {"first": ia.get(k), "second": ib.get(k), "same": ia.get(k) == ib.get(k)} for k in HASHES}
    same = bool(ia.get("raw_sha256")) and ia.get("raw_sha256") == ib.get("raw_sha256")
    manifests = [compare(n, a.first, a.second, a.out) for n in ("install.manifest", "staging.manifest")]
    rec = {"same_image": same, "hashes": hashes, "manifests": manifests}
    with open(os.path.join(a.out, "repro.json"), "w", encoding="utf-8") as f:
        json.dump(rec, f, indent=1)
        f.write("\n")
    md = [f"### {a.title}", ""]
    md.append(f"The two raw images are {'the same' if same else 'not the same'}.")
    md.append("")
    md.append("| Hash | First | Second | Same |")
    md.append("| --- | --- | --- | --- |")
    for k, h in hashes.items():
        md.append(f"| {k} | {(h['first'] or '')[:16]} | {(h['second'] or '')[:16]} | {'yes' if h['same'] else 'no'} |")
    for m in manifests:
        md.append("")
        if m.get("missing"):
            md.append(f"{m['name']}: not in both records.")
            continue
        md.append(
            f"{m['name']}: {m['entries']} entries. {len(m['content'])} differ in content, "
            f"{len(m['metadata'])} only in mode, owner, size or link target, {len(m['only_first'])} only in the first "
            f"and {len(m['only_second'])} only in the second."
        )
        listed = [("content", p) for p in m["content"]] + [("metadata", p) for p in m["metadata"]]
        listed += [("first only", p) for p in m["only_first"]] + [("second only", p) for p in m["only_second"]]
        if listed:
            md.append("")
            md.append("```")
            md.extend(f"{k}\t{p}" for k, p in listed[:200])
            if len(listed) > 200:
                md.append(f"... and {len(listed) - 200} more")
            md.append("```")
    with open(os.path.join(a.out, "repro.md"), "w", encoding="utf-8") as f:
        f.write("\n".join(md) + "\n")
    print("\n".join(md))
    return 0 if same else 1


if __name__ == "__main__":
    sys.exit(main())
