#!/usr/bin/env python3
"""Compare the kernel profiles of R and G (bench-profile.yml).

    profile-table.py R_DIR G_DIR [TOP]

R_DIR and G_DIR hold the NAME.prof files of guest/rb-rc/profile.sh and its bench.tsv. For each
benchmark the script prints the time of each side, the samples of each class, and the TOP
functions (25 by default) with the most R samples or G samples. The functions are sorted by R
minus G, so the functions where R spends more time come first. Both sides do the same work, so
the sample counts compare directly.
"""

import os
import sys


def read_prof(path):
    classes, funcs = {}, {}
    if not os.path.exists(path):
        return None
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            parts = line.split()
            if len(parts) == 3 and parts[0] == "N":
                classes[parts[1]] = int(parts[2])
            elif len(parts) == 3 and parts[0] == "F":
                name = parts[2].split("`", 1)[-1]
                funcs[name] = funcs.get(name, 0) + int(parts[1])
    return classes, funcs


def read_tsv(path):
    times = {}
    if os.path.exists(path):
        with open(path, encoding="utf-8") as f:
            for line in f:
                parts = line.rstrip("\n").split("\t")
                if len(parts) >= 3:
                    times[parts[1]] = parts[2]
    return times


def main():
    rdir, gdir = sys.argv[1], sys.argv[2]
    top = int(sys.argv[3]) if len(sys.argv) > 3 else 25
    rt, gt = read_tsv(os.path.join(rdir, "bench.tsv")), read_tsv(os.path.join(gdir, "bench.tsv"))
    names = sorted({n[:-5] for d in (rdir, gdir) for n in os.listdir(d) if n.endswith(".prof")})
    for name in names:
        r = read_prof(os.path.join(rdir, name + ".prof"))
        g = read_prof(os.path.join(gdir, name + ".prof"))
        print(f"### {name}")
        print()
        print(f"time: R {rt.get(name, '-')} s, G {gt.get(name, '-')} s")
        if r is None or g is None:
            print("no profile of one side")
            print()
            continue
        for c in ("kernel", "user", "idle"):
            print(f"{c} samples: R {r[0].get(c, 0)}, G {g[0].get(c, 0)}")
        print()
        rf, gf = r[1], g[1]
        keys = sorted(rf, key=rf.get, reverse=True)[:top] + sorted(gf, key=gf.get, reverse=True)[:top]
        keys = sorted(set(keys), key=lambda k: rf.get(k, 0) - gf.get(k, 0), reverse=True)
        print("```")
        print(f"{'R':>8} {'G':>8} {'R-G':>8}  function")
        for k in keys:
            a, b = rf.get(k, 0), gf.get(k, 0)
            print(f"{a:>8} {b:>8} {a - b:>8}  {k}")
        print("```")
        print()


if __name__ == "__main__":
    main()
