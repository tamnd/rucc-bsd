#!/usr/bin/env python3
"""What rucc's file-scope asm reader saves on NetBSD userland (docs/plan 07.3).

    scripts/reader-cost.py --log SHIM_TSV --rucc RUCC --target T --gnuc V --sysroot D --out DIR
                   [--budget SECONDS] [--jobs N]

SHIM_TSV is the log of an R TOOLDIR made by "overlay.sh ref": one line for each call of the C
compiler, with the exit status, the directory, the slot and the arguments. Each C unit that
compiled there (slot cc, -c, one .c input) is compiled again by rucc in its own directory, as the
rucc shim would compile it, once with the reader on and once with -frucc-asm-reader=off. The
order of the two is random for each unit. The time of a compile is the user and system time of
the child process.

Each unit is also preprocessed, and the preprocessed text is searched for asm blocks at file
scope. For a unit that has them, the script also times the preprocessed text with the reader off,
with and without its file-scope blocks. That difference is all that the assembler path costs the
unit, so it is the most that any reader could save.

The units run in a random order with a fixed seed until the budget is spent. The script writes
units.tsv, one line for each unit, and summary.md to DIR.
"""

import argparse
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import threading
import time
from concurrent.futures import ThreadPoolExecutor

# Options whose value is the next argument.
VALUED = {
    "-o", "-I", "-D", "-U", "-include", "-imacros", "-isystem", "-iquote", "-idirafter",
    "-MF", "-MT", "-MQ", "-x", "-Xlinker", "-Xassembler", "-Xpreprocessor", "--param",
    "-isysroot", "-iprefix", "-iwithprefix", "-iwithprefixbefore", "-L", "-l", "-T", "-u",
    "-aux-info", "-dumpbase", "-dumpdir",
}
# Options the replay drops, with the number of arguments each takes.
DROP = {"-MD": 0, "-MMD": 0, "-MP": 0, "-MF": 1, "-MT": 1, "-MQ": 1, "-o": 1, "-c": 0}
OFF = "-frucc-asm-reader=off"
TIMEOUT = 300


def units(log):
    """The C units of the log: (directory, arguments, source), each once."""
    seen = set()
    out = []
    with open(log, encoding="utf-8", errors="surrogateescape") as f:
        for line in f:
            fields = line.rstrip("\n").split("\t")
            if len(fields) < 4 or fields[0] != "0" or fields[2] != "cc":
                continue
            cwd, args = fields[1], fields[3:]
            if "-c" not in args or "-E" in args or "-S" in args:
                continue
            sources = []
            skip = False
            for a in args:
                if skip:
                    skip = False
                    continue
                if a in VALUED:
                    skip = True
                    continue
                if not a.startswith("-") and a.endswith(".c"):
                    sources.append(a)
            if len(sources) != 1:
                continue
            key = (cwd, tuple(args))
            if key in seen:
                continue
            seen.add(key)
            out.append((cwd, args, sources[0]))
    return out


def replay_args(args):
    """The arguments without -c, -o, the dependency options and -Werror."""
    out = []
    i = 0
    while i < len(args):
        a = args[i]
        if a in DROP:
            i += 1 + DROP[a]
            continue
        if a == "-Werror" or a.startswith("-Werror=") or a.startswith("-Wp,-M"):
            i += 1
            continue
        out.append(a)
        i += 1
    return out


def run(cmd, cwd):
    """Run cmd and return (exit status, CPU seconds, standard error)."""
    with tempfile.TemporaryFile() as err:
        try:
            p = subprocess.Popen(cmd, cwd=cwd, stdin=subprocess.DEVNULL,
                                 stdout=subprocess.DEVNULL, stderr=err)
        except OSError as e:
            return 127, 0.0, str(e)
        timer = threading.Timer(TIMEOUT, p.kill)
        timer.start()
        _, status, usage = os.wait4(p.pid, 0)
        timer.cancel()
        p.returncode = os.waitstatus_to_exitcode(status)
        err.seek(0)
        text = err.read().decode("utf-8", "replace")
        return p.returncode, usage.ru_utime + usage.ru_stime, text


IDENT = re.compile(r"[A-Za-z_][A-Za-z_0-9]*")


def file_scope_asm(text):
    """The spans of the asm blocks at file scope in preprocessed C.

    A block starts with asm, __asm or __asm__ at brace and parenthesis depth 0, right after the
    start of the file, a semicolon or a closing brace, so that an asm label on a declaration is
    not one. It ends after the closing parenthesis and the semicolon.
    """
    spans = []
    depth = 0
    paren = 0
    last = ";"
    i = 0
    n = len(text)
    while i < n:
        c = text[i]
        if c == "#" and (i == 0 or text[i - 1] == "\n"):
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
            continue
        if c in "\"'":
            j = i + 1
            while j < n and text[j] != c:
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            last = c
            continue
        if c == "/" and text.startswith("/*", i):
            j = text.find("*/", i + 2)
            i = n if j < 0 else j + 2
            continue
        if c.isspace():
            i += 1
            continue
        m = IDENT.match(text, i)
        if m:
            word = m.group(0)
            if (word in ("asm", "__asm", "__asm__") and depth == 0 and paren == 0
                    and last in (";", "}")):
                j = m.end()
                while j < n and text[j].isspace():
                    j += 1
                end = block_end(text, j) if j < n and text[j] == "(" else -1
                if end > 0:
                    spans.append((i, end))
                    i = end
                    last = ";"
                    continue
            last = "w"
            i = m.end()
            continue
        if c == "{":
            depth += 1
        elif c == "}":
            depth -= 1
        elif c == "(":
            paren += 1
        elif c == ")":
            paren -= 1
        last = c
        i += 1
    return spans


def block_end(text, i):
    """The end of an asm block whose parenthesis opens at i, after its semicolon, or -1."""
    n = len(text)
    level = 0
    while i < n:
        c = text[i]
        if c in "\"'":
            j = i + 1
            while j < n and text[j] != c:
                j += 2 if text[j] == "\\" else 1
            i = j + 1
            continue
        if c == "(":
            level += 1
        elif c == ")":
            level -= 1
            if level == 0:
                j = i + 1
                while j < n and text[j].isspace():
                    j += 1
                return j + 1 if j < n and text[j] == ";" else -1
        i += 1
    return -1


def one(unit, base, work, seed):
    """Measure one unit and return its line of units.tsv."""
    cwd, args, source = unit
    rest = replay_args(args)
    tmp = tempfile.mkdtemp(dir=work)
    try:
        obj = {"on": os.path.join(tmp, "on.o"), "off": os.path.join(tmp, "off.o")}
        times = {}
        status = {}
        errors = {}
        order = ["on", "off"]
        random.Random(f"{seed}:{cwd}:{source}").shuffle(order)
        for side in order:
            extra = [OFF] if side == "off" else []
            status[side], times[side], errors[side] = run(
                base + rest + extra + ["-c", "-o", obj[side]], cwd)
        same = "-"
        if status["on"] == 0 and status["off"] == 0:
            with open(obj["on"], "rb") as a, open(obj["off"], "rb") as b:
                same = "yes" if a.read() == b.read() else "no"
        # The preprocessed text, its file-scope blocks and the cost of the assembler path.
        pre = os.path.join(tmp, "unit.i")
        blocks = "-"
        with_asm = without_asm = "-"
        keep = []
        skip = False
        for a in rest:
            if skip:
                skip = False
                continue
            if a in ("-include", "-imacros"):
                skip = True
                continue
            keep.append(a)
        if status["on"] == 0:
            st, _, _ = run(base + rest + ["-E", "-o", pre], cwd)
            if st == 0:
                with open(pre, encoding="utf-8", errors="surrogateescape") as f:
                    text = f.read()
                spans = file_scope_asm(text)
                blocks = str(len(spans))
                if spans:
                    stripped = os.path.join(tmp, "stripped.i")
                    parts = []
                    at = 0
                    for s, e in spans:
                        parts.append(text[at:s])
                        parts.append("\n" * text.count("\n", s, e))
                        at = e
                    parts.append(text[at:])
                    with open(stripped, "w", encoding="utf-8", errors="surrogateescape") as f:
                        f.write("".join(parts))
                    keep = [a for a in keep if a != source]
                    a_st, a_t, _ = run(base + keep + [OFF, "-c", "-o", obj["on"], pre], cwd)
                    b_st, b_t, _ = run(base + keep + [OFF, "-c", "-o", obj["off"], stripped], cwd)
                    if a_st == 0 and b_st == 0:
                        with_asm, without_asm = f"{a_t:.4f}", f"{b_t:.4f}"
        first = ""
        for side in ("on", "off"):
            if status[side] != 0:
                lines = [l for l in errors[side].splitlines() if "error" in l]
                first = (lines[0] if lines else errors[side].strip()[:200]).replace("\t", " ")
                break
        return "\t".join([
            cwd, source, str(status["on"]), str(status["off"]),
            f"{times['on']:.4f}", f"{times['off']:.4f}", same, blocks, with_asm, without_asm,
            first[:300],
        ])
    finally:
        shutil.rmtree(tmp, ignore_errors=True)


COLUMNS = ["dir", "source", "status_on", "status_off", "cpu_on", "cpu_off", "same_object",
           "file_scope_blocks", "cpu_i_with_asm", "cpu_i_without_asm", "first_error"]


def summarize(rows, total, spent, rucc_version):
    """The summary in Markdown."""
    def f(x):
        return float(x)

    ok = [r for r in rows if r[2] == "0" and r[3] == "0"]
    failed = len(rows) - len(ok)
    on = sum(f(r[4]) for r in ok)
    off = sum(f(r[5]) for r in ok)
    asm = [r for r in ok if r[7] not in ("-", "0")]
    asm_on = sum(f(r[4]) for r in asm)
    asm_off = sum(f(r[5]) for r in asm)
    changed = [r for r in asm if r[6] == "no"]
    changed_no_asm = [r for r in ok if r[6] == "no" and r[7] in ("-", "0")]
    bound = [r for r in asm if r[8] != "-"]
    with_asm = sum(f(r[8]) for r in bound)
    without_asm = sum(f(r[9]) for r in bound)
    bound_on = sum(f(r[4]) for r in bound)

    def pct(a, b):
        return f"{100.0 * a / b:.2f}%" if b else "n/a"

    out = []
    out.append("### What the file-scope asm reader saves on NetBSD userland")
    out.append("")
    out.append(f"rucc: `{rucc_version}`")
    out.append("")
    out.append(f"The G build compiled {total} distinct C units. This run measured {len(rows)} of them, "
               f"in a random order, in {spent:.0f} s.")
    out.append(f"rucc compiled {len(ok)} of them with the reader on and off. {failed} failed in one or both.")
    out.append("")
    out.append("| | units | CPU s, reader on | CPU s, reader off | off minus on |")
    out.append("|---|---|---|---|---|")
    out.append(f"| every unit | {len(ok)} | {on:.1f} | {off:.1f} | {off - on:+.1f} ({pct(off - on, on)}) |")
    out.append(f"| units with file-scope asm | {len(asm)} | {asm_on:.1f} | {asm_off:.1f} | "
               f"{asm_off - asm_on:+.1f} ({pct(asm_off - asm_on, on)} of every unit) |")
    out.append("")
    out.append(f"Units with file-scope asm whose object changes when the reader is off: {len(changed)} of {len(asm)}.")
    out.append(f"Units with no file-scope asm whose object changes: {len(changed_no_asm)}.")
    out.append("")
    out.append("The most any reader could save is what the assembler path costs. For the "
               f"{len(bound)} units with file-scope asm whose preprocessed text compiled with the "
               f"reader off, with and without the blocks: {with_asm:.1f} s and {without_asm:.1f} s, "
               f"a difference of {with_asm - without_asm:+.1f} s. That is {pct(with_asm - without_asm, bound_on)} "
               f"of the time of those units and {pct(with_asm - without_asm, on)} of the time of every unit.")
    out.append("")
    out.append("The reader saves: " + pct(asm_off - asm_on, on) + " of the build's compile time.")
    return "\n".join(out) + "\n"


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--log", required=True)
    p.add_argument("--rucc", required=True)
    p.add_argument("--target", required=True)
    p.add_argument("--gnuc", required=True)
    p.add_argument("--sysroot", required=True)
    p.add_argument("--out", required=True)
    p.add_argument("--budget", type=float, default=7200)
    p.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    p.add_argument("--seed", default="07.3")
    a = p.parse_args()

    all_units = units(a.log)
    random.Random(a.seed).shuffle(all_units)
    os.makedirs(a.out, exist_ok=True)
    work = tempfile.mkdtemp(prefix="reader-cost-")
    version = subprocess.run([a.rucc, "--version"], capture_output=True, text=True,
                             check=False).stdout.splitlines()
    version = version[0] if version else "unknown"
    base = [a.rucc, f"--target={a.target}", f"-fgnuc-version={a.gnuc}", f"--sysroot={a.sysroot}"]

    start = time.monotonic()
    rows = []
    with open(os.path.join(a.out, "units.tsv"), "w", encoding="utf-8",
              errors="surrogateescape") as tsv:
        tsv.write("\t".join(COLUMNS) + "\n")
        with ThreadPoolExecutor(max_workers=a.jobs) as pool:
            pending = set()
            it = iter(all_units)
            done = False
            while pending or not done:
                while not done and len(pending) < 2 * a.jobs:
                    if time.monotonic() - start > a.budget:
                        done = True
                        break
                    u = next(it, None)
                    if u is None:
                        done = True
                        break
                    pending.add(pool.submit(one, u, base, work, a.seed))
                finished = [f for f in pending if f.done()]
                if not finished:
                    time.sleep(0.05)
                    continue
                for f in finished:
                    pending.discard(f)
                    line = f.result()
                    tsv.write(line + "\n")
                    rows.append(line.split("\t"))
                if len(rows) % 500 < len(finished):
                    print(f"reader-cost: {len(rows)} of {len(all_units)} units, "
                          f"{time.monotonic() - start:.0f} s", flush=True)
    shutil.rmtree(work, ignore_errors=True)
    text = summarize(rows, len(all_units), time.monotonic() - start, version)
    with open(os.path.join(a.out, "summary.md"), "w", encoding="utf-8") as f:
        f.write(text)
    sys.stdout.write(text)


if __name__ == "__main__":
    main()
