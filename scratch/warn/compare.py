#!/usr/bin/env python3
"""Scratch: compare the warnings of GCC and rucc per unit, from the records of shim-dual.sh.

    compare.py DIR NAMED.rs SUMMARY.md DETAILS.txt

rucc prints its own code, such as [E0785], where GCC prints the option. NAMED.rs is
crates/rucc-diag/src/named.rs of the rucc that ran, whose OPTIONS table maps a code to its option.
"""

import collections
import os
import re
import sys

DIAG = re.compile(
    r"^(?P<file>[^:\s][^:]*):(?P<line>\d+):(?:(?P<col>\d+):)? "
    r"(?P<kind>warning|error|fatal error): (?P<msg>.*?)(?: \[(?P<opt>-W[^\]]+|[EW]\d{4})\])?$"
)
DRIVER = re.compile(r"^(?P<prog>[\w.+-]+): (?P<kind>warning|error|fatal error): (?P<msg>.*?)(?: \[(?P<opt>-W[^\]]+|[EW]\d{4})\])?$")
CODES = {}


def option(opt):
    """The option name of a diagnostic, or the rucc code when the code has none."""
    if opt is None:
        return None
    if not opt.startswith("-W"):
        return CODES.get(opt, opt)
    opt = opt[2:]
    if opt.startswith("error="):
        opt = opt[len("error="):]
    return opt.split("=", 1)[0]


def named(opt):
    return opt is not None and not re.fullmatch(r"[EW]\d{4}", opt)


def shape(msg):
    msg = re.sub(r"'[^']*'", "'_'", msg)
    msg = re.sub(r"\"[^\"]*\"", '"_"', msg)
    msg = re.sub(r"\d+", "N", msg)
    return msg[:90]


def read(path):
    diags, driver = [], []
    try:
        text = open(path, errors="replace").read()
    except FileNotFoundError:
        return diags, driver
    for line in text.splitlines():
        m = DIAG.match(line)
        if m:
            diags.append((os.path.basename(m["file"]), int(m["line"]), m["kind"], option(m["opt"]), m["msg"], line))
            continue
        m = DRIVER.match(line)
        if m:
            driver.append((m["kind"], option(m["opt"]), m["msg"], line))
    return diags, driver


def main():
    top, table, summary, details = sys.argv[1:5]
    for code, name in re.findall(r'\("([EW]\d{4})", "([^"]+)"\)', open(table).read()):
        CODES.setdefault(code, name)
    units = 0
    rucc_failed = 0
    try:
        with open(os.path.join(top, "units.tsv")) as f:
            for line in f:
                units += 1
                if line.split("\t", 1)[0] != "0":
                    rucc_failed += 1
    except FileNotFoundError:
        pass

    extra = collections.defaultdict(list)  # option -> [(unit, line)]
    missing = collections.Counter()
    hard = collections.defaultdict(list)  # message shape -> [(unit, line)]
    drv = collections.defaultdict(list)
    crashes = []
    recdir = os.path.join(top, "u")
    for name in sorted(os.listdir(recdir)) if os.path.isdir(recdir) else []:
        rec = os.path.join(recdir, name)
        cmd = open(os.path.join(rec, "cmd")).read().splitlines()
        cwd, args = cmd[0], cmd[1:]
        src = next((a for a in args if a.endswith(".c")), "?")
        unit = os.path.normpath(os.path.join(cwd, src)).replace("/mnt/nb/src/", "")
        status = int(open(os.path.join(rec, "status")).read().strip() or 0)
        gd, _ = read(os.path.join(rec, "gcc"))
        rd, rdrv = read(os.path.join(rec, "rucc"))
        gkeys = {(f, l, o) for f, l, k, o, m, t in gd if o}
        gfiles = {(f, l) for f, l, k, o, m, t in gd}
        # A GCC warning that no option controls, such as a macro redefined, prints no option, and
        # rucc prints its code where no option controls its warning. The two match on the line.
        gplain = {(f, l) for f, l, k, o, m, t in gd if k == "warning" and o is None}
        rkeys = set()
        for f, l, k, o, m, t in rd:
            if named(o) or k == "warning":
                key = (f, l, o or "(none)")
                if key in rkeys:
                    continue
                rkeys.add(key)
                if key in gkeys or (k == "warning" and not named(o) and (f, l) in gplain):
                    continue
                extra[o or "(none)"].append((unit, t, (f, l) in gfiles))
            else:
                hard[f"{o or ''} {shape(m)}"].append((unit, t))
        for f, l, o in gkeys - rkeys:
            missing[o] += 1
        for k, o, m, t in rdrv:
            drv[(k, o or "", shape(m))].append((unit, t))
        if status >= 124 and not rd and not rdrv:
            crashes.append((unit, status))

    with open(summary, "w") as out:
        p = lambda *a: print(*a, file=out)
        p(f"Units compiled by both: {units}. rucc exit status not 0: {rucc_failed}.")
        p()
        p("#### Warnings rucc gives and GCC does not, by option")
        p()
        p("| Option | Diagnostics | Units | GCC warned on the same line |")
        p("|---|---|---|---|")
        for o, rows in sorted(extra.items(), key=lambda kv: -len(kv[1])):
            p(f"| `{o}` | {len(rows)} | {len({u for u, _, _ in rows})} | {sum(1 for _, _, s in rows if s)} |")
        p()
        p("#### rucc errors without an option, by message")
        p()
        p("| Message | Diagnostics | Units |")
        p("|---|---|---|")
        for m, rows in sorted(hard.items(), key=lambda kv: -len(kv[1]))[:60]:
            p(f"| `{m}` | {len(rows)} | {len({u for u, _ in rows})} |")
        p()
        p("#### rucc driver messages")
        p()
        p("| Kind | Option | Message | Units |")
        p("|---|---|---|---|")
        for (k, o, m), rows in sorted(drv.items(), key=lambda kv: -len(kv[1]))[:40]:
            p(f"| {k} | `{o}` | `{m}` | {len({u for u, _ in rows})} |")
        p()
        p(f"rucc crashed or timed out without a message: {len(crashes)} units.")
        p()
        p("#### Warnings GCC gives and rucc does not, by option")
        p()
        p("| Option | Diagnostics |")
        p("|---|---|")
        for o, n in missing.most_common():
            p(f"| `{o}` | {n} |")

    with open(details, "w") as out:
        p = lambda *a: print(*a, file=out)
        for o, rows in sorted(extra.items(), key=lambda kv: -len(kv[1])):
            p(f"== extra -W{o}: {len(rows)}")
            seen = set()
            for u, t, s in rows:
                if u in seen and len(seen) < 40:
                    continue
                seen.add(u)
                p(f"  {u}: {t}{'  (gcc warned on the line)' if s else ''}")
                if len(seen) >= 40:
                    break
        for m, rows in sorted(hard.items(), key=lambda kv: -len(kv[1])):
            p(f"== error {m}: {len(rows)}")
            for u, t in rows[:15]:
                p(f"  {u}: {t}")
        for (k, o, m), rows in sorted(drv.items(), key=lambda kv: -len(kv[1])):
            p(f"== driver {k} {o} {m}: {len(rows)}")
            for u, t in rows[:5]:
                p(f"  {u}: {t}")
        for u, s in crashes:
            p(f"== crash {s}: {u}")


if __name__ == "__main__":
    main()
