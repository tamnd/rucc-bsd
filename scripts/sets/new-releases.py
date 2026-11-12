#!/usr/bin/env python3
"""Print the release pins of a new pins.toml that an old one does not have (plan 04.7).

    new-releases.py OLD_PINS_TOML NEW_PINS_TOML

One line per new release pin, "SYSTEM NAME", such as "freebsd freebsd-15.2", in the order of the
new file. sets.yml starts the G build of each one, so a release joins on the day it is tagged.
"""
import sys
import tomllib


def releases(path):
    with open(path, "rb") as f:
        pins = tomllib.load(f)
    return [(p["system"], p["name"]) for p in pins.get("pin", []) if p.get("kind") == "release"]


def main():
    if len(sys.argv) != 3:
        print("usage: new-releases.py OLD_PINS_TOML NEW_PINS_TOML", file=sys.stderr)
        return 2
    old = {name for _, name in releases(sys.argv[1])}
    for system, name in releases(sys.argv[2]):
        if name not in old:
            print(system, name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
