#!/usr/bin/env python3
"""Print the line of conf/freebsd/pins.conf for a FreeBSD pin of pins.toml (plan 04.5 and 04.7).

    pin-line.py PINS_TOML NAME

A release pin with a src.txz tarball gives "NAME txz URL SHA256", and a pin with a repo and a
commit gives "NAME git REPO COMMIT". So fetch.sh can fetch a pin that rb sets wrote on the day a
release was tagged, before anyone adds it to pins.conf. Exits 2 when there is no such pin or it
has no source that fetch.sh knows.
"""
import sys
import tomllib


def line(pins, name):
    for pin in pins.get("pin", []):
        if pin.get("name") != name or pin.get("system") != "freebsd":
            continue
        for tarball in pin.get("tarball", []):
            if tarball.get("name") == "src.txz" and tarball.get("sha256"):
                return f"{name}\ttxz\t{tarball['url']}\t{tarball['sha256']}"
        if pin.get("repo") and pin.get("commit"):
            return f"{name}\tgit\t{pin['repo']}\t{pin['commit']}"
        return None
    return None


def main():
    if len(sys.argv) != 3:
        print("usage: pin-line.py PINS_TOML NAME", file=sys.stderr)
        return 2
    with open(sys.argv[1], "rb") as f:
        pins = tomllib.load(f)
    found = line(pins, sys.argv[2])
    if found is None:
        print(f"no FreeBSD pin {sys.argv[2]} with a source in {sys.argv[1]}", file=sys.stderr)
        return 2
    print(found)
    return 0


if __name__ == "__main__":
    sys.exit(main())
