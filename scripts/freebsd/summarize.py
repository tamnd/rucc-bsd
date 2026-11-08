#!/usr/bin/env python3
"""Write the summary of one FreeBSD run in the format of results/freebsd/README.md.

    summarize.py --build DIR [--boot NAME=DIR ...] [--published FILE] --out DIR

--build is the record of the build job: source.txt, toolchain.txt, phases.tsv, image.txt.
--boot is the OUTDIR of one boot.sh, named for its firmware. --published is the JSON that
published.py wrote. The script writes summary.json, cases.tsv when kyua ran, and summary.md for the
step summary.
"""

import argparse
import json
import os
import re
import sys

CASE = re.compile(
    r"^(\S+:\S+)\s+->\s+(passed|failed|broken|skipped|expected_failure)\b.*?(?:\[([0-9.]+)s\])?\s*$"
)
RESULTS = ["passed", "failed", "broken", "skipped", "expected_failure"]


def read(path):
    try:
        with open(path, encoding="utf-8", errors="replace") as f:
            return f.read()
    except OSError:
        return None


def build_record(d):
    rec = {}
    src = read(os.path.join(d, "source.txt"))
    if src:
        pin, kind, source, h = src.strip().split("\t")
        rec["source"] = {"pin": pin, "kind": kind, "url": source, "hash": h}
    tc = read(os.path.join(d, "toolchain.txt"))
    if tc:
        pkgs = {}
        section = None
        for line in tc.splitlines():
            if line.startswith("# "):
                section = line
                continue
            if section and section.startswith("# packages") and line.strip():
                name, version = line.split()[:2]
                pkgs[name] = version
        rec["toolchain"] = pkgs
    phases = read(os.path.join(d, "phases.tsv"))
    if phases:
        rec["phases"] = {}
        for line in phases.splitlines():
            name, status, seconds, size_kb, used_kb = line.split("\t")
            rec["phases"][name] = {
                "status": int(status),
                "seconds": int(seconds),
                "size_kb": int(size_kb),
                "disk_used_kb": int(used_kb),
            }
    image = read(os.path.join(d, "image.txt"))
    if image:
        rec["image"] = {}
        for line in image.splitlines():
            k, v = line.split(" ", 1)
            rec["image"][k] = int(v) if v.isdigit() else v
    return rec


def kyua_cases(report):
    cases = {}
    for line in report.splitlines():
        m = CASE.match(line)
        if m:
            cases[m.group(1)] = (m.group(2), float(m.group(3) or 0))
    return cases


def boot_record(d):
    rec = json.loads(read(os.path.join(d, "boot.json")) or "{}")
    out = os.path.join(d, "results", "out")
    smoke = read(os.path.join(out, "smoke", "results"))
    if smoke:
        rec["smoke"] = {n: r for r, n in (line.split(" ", 1) for line in smoke.splitlines() if line)}
    report = read(os.path.join(out, "kyua", "report.txt"))
    cases = None
    if report:
        cases = kyua_cases(report)
        counts = {r: 0 for r in RESULTS}
        for result, _ in cases.values():
            counts[result] += 1
        counts["total"] = len(cases)
        rec["kyua"] = {
            "counts": counts,
            "seconds": int((read(os.path.join(out, "kyua", "seconds")) or "0").strip() or 0),
            "exit": (read(os.path.join(out, "kyua", "exit")) or "").strip(),
            "excluded": (read(os.path.join(out, "kyua", "excluded")) or "").split(),
        }
    sysctl = read(os.path.join(out, "sysctl.txt"))
    if sysctl:
        for line in sysctl.splitlines():
            if line.startswith("kern.compiler_version:"):
                rec["compiler_version"] = line.split(":", 1)[1].strip()
    return rec, cases


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--build", required=True)
    ap.add_argument("--boot", action="append", default=[])
    ap.add_argument("--published")
    ap.add_argument("--conf", default="graded")
    ap.add_argument("--side", default="G")
    ap.add_argument("--run", default=os.environ.get("GITHUB_RUN_ID", ""))
    ap.add_argument("--out", required=True)
    a = ap.parse_args()

    s = {"system": "freebsd", "row": "amd64", "conf": a.conf, "side": a.side, "run": a.run}
    s.update(build_record(a.build))
    s["boots"] = {}
    all_cases = None
    for b in a.boot:
        name, d = b.split("=", 1)
        rec, cases = boot_record(d)
        s["boots"][name] = rec
        if cases is not None:
            all_cases = cases
    if a.published and os.path.exists(a.published):
        pub = json.loads(read(a.published))
        s["published"] = pub
        k = next((b["kyua"] for b in s["boots"].values() if "kyua" in b), None)
        if k and pub.get("total"):
            c = k["counts"]
            s["gap"] = {
                "total": c["total"] - pub["total"],
                "total_percent": round(100.0 * (c["total"] - pub["total"]) / pub["total"], 2),
                "failed_and_broken": c["failed"] + c["broken"] - pub["failed"],
                "skipped": c["skipped"] - pub["skipped"],
            }

    os.makedirs(a.out, exist_ok=True)
    with open(os.path.join(a.out, "summary.json"), "w") as f:
        json.dump(s, f, indent=1, sort_keys=True)
        f.write("\n")
    if all_cases is not None:
        with open(os.path.join(a.out, "cases.tsv"), "w") as f:
            for name in sorted(all_cases):
                result, secs = all_cases[name]
                f.write(f"{name}\t{result}\t{secs:.3f}\n")

    md = []
    src = s.get("source", {})
    md.append(f"### FreeBSD {src.get('pin', '?')} amd64 {a.conf}, {a.side}")
    md.append("")
    if src:
        md.append(f"Source: `{src['url']}` at `{src['hash']}`.")
    if s.get("toolchain"):
        md.append("Toolchain: " + ", ".join(f"`{n} {v}`" for n, v in sorted(s["toolchain"].items())) + ".")
    md.append("")
    if s.get("phases"):
        md.append("| Phase | Status | Minutes | Size (GB) | Disk used (GB) |")
        md.append("|---|---|---|---|---|")
        for n, p in s["phases"].items():
            md.append(
                f"| {n} | {p['status']} | {p['seconds'] / 60:.1f} | {p['size_kb'] / 1048576:.2f} "
                f"| {p['disk_used_kb'] / 1048576:.2f} |"
            )
        md.append("")
    if s.get("image"):
        im = s["image"]
        md.append(
            f"Image: {im.get('raw_bytes', 0) / 2**30:.1f} GB raw, "
            f"{im.get('zst_bytes', 0) / 2**30:.2f} GB compressed."
        )
        md.append("")
    for name, b in s["boots"].items():
        line = f"Boot {name}: {b.get('result')}, RB-READY after {b.get('ready_seconds')} s, {b.get('seconds')} s in all."
        md.append(line)
        if "smoke" in b:
            passed = sum(1 for r in b["smoke"].values() if r == "pass")
            md.append(
                f"Smoke: {passed}/{len(b['smoke'])} passed"
                + "".join(f", {n} failed" for n, r in b["smoke"].items() if r != "pass")
                + "."
            )
        if "kyua" in b:
            c = b["kyua"]["counts"]
            md.append(
                f"kyua: {c['total']} cases, {c['passed']} passed, {c['failed']} failed, {c['broken']} broken, "
                f"{c['skipped']} skipped, {c['expected_failure']} expected failures, in {b['kyua']['seconds'] / 60:.0f} minutes."
            )
        md.append("")
    if "published" in s:
        p = s["published"]
        md.append(
            f"Published: {p['job']} #{p['build']} at `{p.get('commit', '?')[:12]}`: {p['total']} cases, "
            f"{p['failed']} failed or broken, {p['skipped']} skipped."
        )
        if "gap" in s:
            g = s["gap"]
            md.append(
                f"Gap: {g['total']:+d} cases ({g['total_percent']:+.2f}%), {g['failed_and_broken']:+d} failed or broken, "
                f"{g['skipped']:+d} skipped."
            )
    with open(os.path.join(a.out, "summary.md"), "w") as f:
        f.write("\n".join(md) + "\n")
    print("\n".join(md))
    return 0


if __name__ == "__main__":
    sys.exit(main())
