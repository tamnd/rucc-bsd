#!/usr/bin/env python3
"""Write the manifest of a NetBSD release directory, to compare two builds (plan 16.1).

    sets-manifest.py RELEASEDIR OUT

RELEASEDIR is the -R directory of build.sh. The manifest has one line per file under
<machine>/binary/sets and <machine>/binary/kernel:

    file PATH SIZE SHA256

and, for each set (a .tgz or .tar.xz file), one line per member:

    member SET NAME TYPE MODE UID GID MTIME SIZE SHA256-OR-LINK

The lines are sorted. Two builds of one tree with build.sh -P must give the same manifest.
"""

import hashlib
import os
import sys
import tarfile


def sha256(f):
    h = hashlib.sha256()
    for chunk in iter(lambda: f.read(1 << 20), b""):
        h.update(chunk)
    return h.hexdigest()


def main():
    rel, out = sys.argv[1], sys.argv[2]
    lines = []
    for top, _, files in os.walk(rel):
        rel_top = os.path.relpath(top, rel)
        parts = rel_top.split(os.sep)
        if len(parts) < 3 or parts[1] != "binary" or parts[2] not in ("sets", "kernel"):
            continue
        for name in sorted(files):
            path = os.path.join(top, name)
            rp = os.path.join(rel_top, name)
            with open(path, "rb") as f:
                lines.append(f"file {rp} {os.path.getsize(path)} {sha256(f)}")
            if not name.endswith((".tgz", ".tar.xz")):
                continue
            with tarfile.open(path) as t:
                for m in t:
                    if m.isfile():
                        what = sha256(t.extractfile(m))
                    elif m.issym() or m.islnk():
                        what = "->" + m.linkname
                    else:
                        what = "-"
                    kind = {tarfile.REGTYPE: "f", tarfile.DIRTYPE: "d", tarfile.SYMTYPE: "l", tarfile.LNKTYPE: "h"}
                    lines.append(
                        f"member {name} {m.name} {kind.get(m.type, m.type.decode())} {m.mode:o} {m.uid} {m.gid} "
                        f"{int(m.mtime)} {m.size} {what}"
                    )
    lines.sort()
    with open(out, "w", encoding="utf-8") as f:
        f.write("\n".join(lines) + "\n")
    print(f"{out}: {sum(1 for x in lines if x.startswith('file '))} files, "
          f"{sum(1 for x in lines if x.startswith('member '))} members")
    return 0


if __name__ == "__main__":
    sys.exit(main())
