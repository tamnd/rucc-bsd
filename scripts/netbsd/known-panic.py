#!/usr/bin/env python3
"""Print the issue of a known NetBSD race that made a boot of rb boot panic, and exit 0. Exit 1
when the boot did not panic in a known way.

    known-panic.py OUT

OUT is the --out directory of rb boot, with boot.json and console.log. The known panics occur
with the G and the R kernel:

    65   the assertion on MDL_FPU_IN_CPU in fpu_lwp_area, in an x86_xstate case of the ptrace
         tests.
    170  a fault in aio_job_mark_complete or aiost_process_singleton of the aio service thread,
         after the process freed the job, in a lib/libc/sys/t_aio_ case.
"""

import json
import sys
from pathlib import Path

AIO_FRAMES = ("aio_job_mark_complete", "aiost_process_singleton", "aiost_entry")


def known(seen, console):
    panic = seen.get("panic") or ""
    case = seen.get("panic_case") or ""
    if "MDL_FPU_IN_CPU" in panic and ":x86_xstate_" in case:
        return 65
    if case.startswith("lib/libc/sys/t_aio_"):
        # The traceback after the last panic line names the aio service thread.
        tail = console[console.rfind("panic: ") :] if "panic: " in console else ""
        if any(f"{f}() at" in tail for f in AIO_FRAMES):
            return 170
    return None


def main() -> int:
    try:
        out = Path(sys.argv[1])
        seen = json.loads((out / "boot.json").read_text(encoding="utf-8")).get("seen") or {}
    except (IndexError, OSError, ValueError):
        return 1
    try:
        console = (out / "console.log").read_text(encoding="utf-8", errors="replace")
    except OSError:
        console = ""
    issue = known(seen, console)
    if issue is None:
        return 1
    print(issue)
    return 0


if __name__ == "__main__":
    sys.exit(main())
