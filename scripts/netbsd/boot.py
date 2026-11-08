#!/usr/bin/env python3
"""Boot a NetBSD disk image under QEMU, log in on the serial console and run guest scripts.

    boot.py --image DISK.img --out DIR [--suite smoke] [--suite atf:only:net] ...

The image must have its console on com0 (prepare-image.sh). A second, empty virtio disk carries
the scripts of scripts/netbsd/guest in, as a tar archive with no file system, and carries the
results out the same way. No network is used to report, because the network stack is under test.

The script logs in as root, as anita does. The live image has no root password. Each suite is a
command line run in the guest; the console prints a marker with its exit status when it ends.

Files written to DIR:
    console.log    everything the serial console printed
    boot.json      the stages reached, their times, and the exit status of each suite
    results/       the contents of the guest's /rb/out after the last suite

The exit status is 0 when the guest booted, every suite ran to its end and the guest powered off.
A suite that reports failures still counts as run; the counts are in results/.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tarfile
import tempfile
import time

import pexpect

HERE = os.path.dirname(os.path.abspath(__file__))
PROMPT = "RBPROMPT# "


class Run:
    def __init__(self, out):
        self.out = out
        self.t0 = time.time()
        self.stages = []
        self.suites = []
        self.result = "unknown"

    def stage(self, name):
        took = round(time.time() - self.t0, 1)
        self.stages.append({"stage": name, "seconds": took})
        print(f"\n[boot.py] {name} at {took} s", flush=True)

    def save(self):
        with open(os.path.join(self.out, "boot.json"), "w") as f:
            json.dump({"result": self.result, "stages": self.stages, "suites": self.suites}, f,
                      indent=2)
            f.write("\n")


def make_transfer_disk(path, size):
    """Write the guest scripts as a tar archive at the start of an empty raw disk."""
    with open(path, "wb") as f:
        f.truncate(size)
    with tempfile.TemporaryDirectory() as tmp:
        tar = os.path.join(tmp, "in.tar")
        with tarfile.open(tar, "w", format=tarfile.USTAR_FORMAT) as t:
            t.add(os.path.join(HERE, "guest"), arcname="guest")
        with open(tar, "rb") as src, open(path, "r+b") as dst:
            shutil.copyfileobj(src, dst)


def read_transfer_disk(path, dest):
    os.makedirs(dest, exist_ok=True)
    try:
        with tarfile.open(path, "r:") as t:
            t.extractall(dest, filter="data")
        return True
    except tarfile.TarError as e:
        print(f"[boot.py] no results on the transfer disk: {e}", flush=True)
        return False


def qemu_command(args, transfer):
    cmd = [
        "qemu-system-x86_64",
        "-machine", "q35",
        "-m", args.memory,
        "-smp", str(args.cpus),
        "-display", "none",
        "-serial", "stdio",
        "-monitor", "none",
        "-drive", f"file={args.image},format=raw,if=virtio,cache=unsafe",
        "-drive", f"file={transfer},format=raw,if=virtio,cache=unsafe",
        "-netdev", "user,id=n0",
        "-device", "virtio-net-pci,netdev=n0",
    ]
    if os.access("/dev/kvm", os.R_OK | os.W_OK):
        cmd[1:1] = ["-enable-kvm", "-cpu", "host"]
    else:
        print("[boot.py] /dev/kvm is not usable, running under TCG", flush=True)
    if args.firmware:
        cmd += ["-bios", args.firmware]
    return cmd


def run_command(child, line, timeout):
    """Run one shell command line in the guest and return its exit status."""
    child.sendline(f"{line}; echo RB-STATUS=$?")
    child.expect(r"RB-STATUS=(\d+)", timeout=timeout)
    status = int(child.match.group(1))
    child.expect_exact(PROMPT, timeout=60)
    return status


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--image", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--suite", action="append", default=[],
                    help="smoke, or atf:all, atf:only:DIR,DIR or atf:except:DIR,DIR")
    ap.add_argument("--memory", default="8G")
    ap.add_argument("--cpus", type=int, default=2)
    ap.add_argument("--firmware", default="", help="a -bios file, for example OVMF.fd")
    ap.add_argument("--boot-timeout", type=int, default=1200)
    ap.add_argument("--suite-timeout", type=int, default=5 * 3600)
    args = ap.parse_args()

    os.makedirs(args.out, exist_ok=True)
    run = Run(args.out)
    transfer = os.path.join(args.out, "transfer.img")
    make_transfer_disk(transfer, 4 << 30)

    cmd = qemu_command(args, transfer)
    print("[boot.py] " + " ".join(cmd), flush=True)
    log = open(os.path.join(args.out, "console.log"), "w", encoding="utf-8", errors="replace")

    class Tee:
        def write(self, s):
            log.write(s)
            sys.stdout.write(s)

        def flush(self):
            log.flush()
            sys.stdout.flush()

    child = pexpect.spawn(cmd[0], cmd[1:], encoding="utf-8", codec_errors="replace",
                          timeout=args.boot_timeout, maxread=65536)
    child.logfile_read = Tee()
    status = 1
    try:
        run.stage("qemu started")
        # The first boot of a grown image resizes the root file system and reboots, so the first
        # login prompt can come after a second kernel banner. A panic drops into ddb.
        while True:
            i = child.expect([r"login: ", r"db\{\d+\}> ", r"root device", r"Copyright \(c\)"],
                             timeout=args.boot_timeout)
            if i == 0:
                break
            if i == 1:
                run.stage("panic")
                child.sendline("bt")
                child.expect(r"db\{\d+\}> ", timeout=60)
                run.result = "panic"
                raise SystemExit(1)
            if i == 2:
                run.stage("asked for the root device")
                run.result = "no-root"
                raise SystemExit(1)
            run.stage("kernel banner")
        run.stage("login prompt")
        child.sendline("root")
        child.expect(r"# ", timeout=120)
        # A prompt that the echo of the command that sets it cannot match.
        child.sendline("PS1='RB''PROMPT# '; export PS1")
        child.expect_exact(PROMPT, timeout=60)
        run.stage("logged in")
        run_command(child, "uname -a", 60)

        # Find the transfer disk: the virtio disk with no wedges on it. ld0 holds the image.
        st = run_command(child, "mkdir -p /rb && cd /rb && tar xf /dev/rld1d && ls guest", 300)
        if st != 0:
            run.result = "no-transfer-disk"
            raise SystemExit(1)
        run.stage("guest scripts in")

        for suite in args.suite:
            name, _, spec = suite.partition(":")
            if name == "smoke":
                line = "sh /rb/guest/smoke.sh /rb/out"
            elif name == "atf":
                mode, _, dirs = spec.partition(":")
                line = f"sh /rb/guest/atf.sh /rb/out {mode} {dirs.replace(',', ' ')}"
            else:
                raise SystemExit(f"unknown suite {suite}")
            started = time.time()
            st = run_command(child, line, args.suite_timeout)
            run.suites.append({"suite": suite, "status": st,
                               "seconds": round(time.time() - started, 1)})
            run.stage(f"suite {suite} ended with {st}")
            run_command(child, "cd /rb/out && tar cf /dev/rld1d . ; cd /", 600)

        child.sendline("sync; shutdown -p now")
        child.expect(pexpect.EOF, timeout=600)
        run.stage("powered off")
        run.result = "ok"
        status = 0
    except pexpect.TIMEOUT:
        run.stage("timeout")
        run.result = "timeout"
    except pexpect.EOF:
        run.stage("qemu exited")
        if run.result == "unknown":
            run.result = "qemu-exited"
    except SystemExit as e:
        if isinstance(e.code, str):
            print(e.code, file=sys.stderr)
    finally:
        if child.isalive():
            child.terminate(force=True)
        log.close()
        read_transfer_disk(transfer, os.path.join(args.out, "results"))
        os.unlink(transfer)
        run.save()
    sys.exit(status)


if __name__ == "__main__":
    main()
