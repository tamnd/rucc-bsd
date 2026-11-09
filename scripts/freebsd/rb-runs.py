#!/usr/bin/env python3
"""Turn the output of boot.sh into the output of rb boot, for rb baseline --from (plan 12.5).

    rb-runs.py BOOTDIR OUTDIR [--image IMG]

BOOTDIR is the OUTDIR of one boot.sh. OUTDIR gets boot.json in the format of rb boot, and a results
directory with the files that rb baseline reads:

    results/out/smoke/results   ->  results/smoke/results
    results/out/kyua/test.log   ->  results/tests/kyua.log
    results/out/dmesg.end       ->  results/dmesg.end
    results/out/dmesg.boot      ->  results/dmesg.boot

The result "done" of boot.sh is "pass" for rb. The suite kyua of boot.sh is the suite tests of rb.
"""

import argparse
import json
import os
import shutil
import sys

FILES = [
    ("smoke/results", "smoke/results"),
    ("kyua/test.log", "tests/kyua.log"),
    ("dmesg.end", "dmesg.end"),
    ("dmesg.boot", "dmesg.boot"),
]
RESULTS = {"done": "pass", "panic": "panic", "timeout": "timeout", "no-done": "no-done", "no-ready": "no-ready"}
SUITES = {"boot": "boot", "smoke": "smoke", "kyua": "tests"}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("boot")
    ap.add_argument("out")
    ap.add_argument("--image", default="")
    a = ap.parse_args()
    with open(os.path.join(a.boot, "boot.json"), encoding="utf-8") as f:
        b = json.load(f)
    result = RESULTS.get(b.get("result", ""), b.get("result", "unknown"))
    asked = [SUITES.get(s, s) for s in b.get("suites", "").split()]
    ready = b.get("ready_seconds")
    done = b.get("done") or None
    panic = b.get("panic") or None
    found = []
    for src, dst in FILES:
        p = os.path.join(a.boot, "results", "out", src)
        if os.path.isfile(p):
            q = os.path.join(a.out, "results", dst)
            os.makedirs(os.path.dirname(q), exist_ok=True)
            shutil.copyfile(p, q)
            found.append(dst)
    outcome = {
        "result": result,
        "image": a.image,
        "firmware": {"bios": "seabios", "uefi": "ovmf"}.get(b.get("firmware", ""), b.get("firmware", "")),
        "accel": "kvm",
        "asked": asked,
        "command": [],
        "restarts": 0,
        "ready_seconds": None if ready is None else float(ready),
        "done_seconds": float(b["seconds"]) if done else None,
        "seconds": float(b.get("seconds", 0)),
        "qemu_exit": None,
        "results": "disk" if found else None,
        "seen": {
            "ready": "RB-READY" if ready is not None else None,
            "conf_from_disk": True,
            "no_disk": False,
            "suites": [],
            "checks": [],
            "case": None,
            "panic": panic,
            "panic_case": None,
            "panic_suite": None,
            "ddb": False,
            "login": False,
            "done": done,
            "archive": None,
        },
    }
    os.makedirs(a.out, exist_ok=True)
    with open(os.path.join(a.out, "boot.json"), "w", encoding="utf-8") as f:
        json.dump(outcome, f, indent=1)
        f.write("\n")
    print(f"{a.boot}: {b.get('result')} -> {result}, suites {' '.join(asked)}, files {' '.join(found) or 'none'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
