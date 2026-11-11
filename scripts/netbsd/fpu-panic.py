#!/usr/bin/env python3
"""Exit 0 if a boot.json of rb boot shows the panic of #65, and 1 if not.

The panic of #65 is the assertion on MDL_FPU_IN_CPU in fpu_lwp_area, in an x86_xstate case of the
ptrace tests. It occurs with the G and the R kernel.

    fpu-panic.py BOOT-JSON
"""

import json
import sys


def main() -> int:
    try:
        with open(sys.argv[1], encoding="utf-8") as f:
            seen = json.load(f).get("seen") or {}
    except (IndexError, OSError, ValueError):
        return 1
    panic = seen.get("panic") or ""
    case = seen.get("panic_case") or ""
    return 0 if "MDL_FPU_IN_CPU" in panic and ":x86_xstate_" in case else 1


if __name__ == "__main__":
    sys.exit(main())
