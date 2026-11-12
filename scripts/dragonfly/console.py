#!/usr/bin/env python3
"""Drive the serial console of the DragonFly builder VM until root has a shell (plan 14.6).

    console.py SOCKET STEPS

The DragonFly release image has no serial console, so the loader menu and the kernel use the
screen. QEMU runs with graphics=off, which makes SeaBIOS copy the screen to the serial port and
read the keyboard from it. This script stops the loader menu with ESC. At the loader prompt it
types the commands of item 1 of the menu, with console=comconsole and boot_serial=YES added. The
live system then runs getty on ttyd0, and root logs in with no password. Each line of the file
STEPS is then run as a command of /bin/sh. A step that exits with a status other than 0 stops the
script.

QEMU writes everything the VM prints to its own log. Timeouts: RB_LOADER_LIMIT seconds for the
loader menu (default 300), RB_LOGIN_LIMIT for the login prompt (default 900) and RB_STEP_LIMIT for
each step (default 600).
"""

import os
import re
import socket
import sys
import time


LOADER = [
    "cd ${base}${default_kernel}",
    "optinclude loader.conf",
    "optinclude loader.conf.local",
    "set console=comconsole",
    "set boot_serial=YES",
    "lunsetif acpi_load hint.acpi.0.disabled",
    "lunsetif ahci_load hint.ahci.disabled",
    "lunsetif ehci_load hint.ehci.0.disabled",
    "lunsetif xhci_load hint.xhci.0.disabled",
    "loadall",
    "boot",
]


class Console:
    def __init__(self, path):
        deadline = time.time() + 60
        while True:
            try:
                self.sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
                self.sock.connect(path)
                break
            except OSError:
                if time.time() > deadline:
                    raise
                time.sleep(1)
        self.buf = ""

    def expect(self, pattern, limit):
        """Wait until the output after the last match has pattern. Return the match or None."""
        rx = re.compile(pattern)
        deadline = time.time() + limit
        while True:
            m = rx.search(self.buf)
            if m:
                self.buf = self.buf[m.end():]
                return m
            left = deadline - time.time()
            if left <= 0:
                return None
            self.sock.settimeout(min(left, 5))
            try:
                data = self.sock.recv(4096)
            except socket.timeout:
                continue
            if not data:
                return None
            # SeaBIOS writes screen updates with escape sequences. Only the text is matched.
            text = data.decode("latin-1")
            text = re.sub(r"\x1b\[[0-9;?]*[A-Za-z]", "", text)
            self.buf = (self.buf + text)[-65536:]

    def send(self, text):
        # The keyboard buffer of the BIOS and the tty input queue are small, so type slowly.
        for ch in text:
            self.sock.sendall(ch.encode("latin-1"))
            time.sleep(0.02)


def fail(what):
    print(f"console.py: {what}", file=sys.stderr)
    sys.exit(1)


def main():
    path, steps = sys.argv[1], sys.argv[2]
    loader_limit = int(os.environ.get("RB_LOADER_LIMIT", "300"))
    login_limit = int(os.environ.get("RB_LOGIN_LIMIT", "900"))
    step_limit = int(os.environ.get("RB_STEP_LIMIT", "600"))
    con = Console(path)

    if not con.expect(r"Booting in \d+ second", loader_limit):
        fail("no loader menu")
    con.send("\x1b")
    if not con.expect(r"OK", 30):
        fail("no loader prompt after ESC")
    # The commands of item 1 of the menu (dloader.menu), with the serial console set after
    # loader.conf is read, so that nothing in it can set the console back.
    for line in LOADER:
        con.send(line + "\r")
        if line == "boot":
            break
        if not con.expect(r"OK", 60):
            fail(f"no loader prompt after {line}")
    print("console.py: the loader boots the kernel with the serial console", flush=True)

    if not con.expect(r"login: ?$|login: ", login_limit):
        fail("no login prompt")
    con.send("root\r")
    # The prompt of the csh of root is "root@HOST:~ # ", and HOST is empty in the live system.
    if not con.expect(r"@[^\r\n]*[#%] ", 60):
        fail("no shell prompt after login")
    # Characters typed while the shell starts can be lost, so the change to /bin/sh is tried again
    # until its prompt comes. exec /bin/sh in /bin/sh does no harm.
    for _ in range(5):
        time.sleep(3)
        con.send("exec /bin/sh\r")
        time.sleep(3)
        con.send("PS1='rb# '\r")
        if con.expect(r"rb# ", 20):
            break
    else:
        fail("no prompt from /bin/sh")

    with open(steps) as f:
        lines = [ln.rstrip("\n") for ln in f if ln.strip() and not ln.startswith("#")]
    for i, line in enumerate(lines):
        print(f"console.py: step {i + 1}: {line[:72]}", flush=True)
        con.send(f"{line}; echo RB-STEP-{i}-$?\r")
        m = con.expect(rf"RB-STEP-{i}-(\d+)", step_limit)
        if not m:
            fail(f"step {i + 1} did not finish")
        if m.group(1) != "0":
            fail(f"step {i + 1} exited with {m.group(1)}")
    print("console.py: done", flush=True)


if __name__ == "__main__":
    main()
