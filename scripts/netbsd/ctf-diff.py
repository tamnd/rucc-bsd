#!/usr/bin/env python3
"""Compare the CTF of two NetBSD kernels, until rb ctf-diff (plan 09.6) does it.

    ctf-diff.py G-KERNEL R-KERNEL [--out DIR]

Each kernel is an ELF file with a .SUNW_ctf section, as nbctfmerge writes it (CTF version 3). The
script decodes the type section of each and writes every type in a form that does not use type
ids: a struct or union with its members, their types and their bit offsets, an enum with its
values, a typedef with what it names. Anonymous types are written in full where they are used.

It prints a markdown report to standard output:
  - the sizes of the sections and the number of types,
  - the named types that are not the same in the two kernels,
  - all types as a multiset, by kind, that only one of the kernels has.

With --out, DIR/ctf-g.txt and DIR/ctf-r.txt get the named types, and DIR/ctf-named.diff and
DIR/ctf-all.diff the differences. The exit status is 0 when the two have the same types, and 1 when
they do not.
"""

import argparse
import os
import struct
import sys
import zlib
from collections import Counter

KIND = {0: 'unknown', 1: 'int', 2: 'float', 3: 'ptr', 4: 'array', 5: 'func', 6: 'struct',
        7: 'union', 8: 'enum', 9: 'forward', 10: 'typedef', 11: 'volatile', 12: 'const',
        13: 'restrict'}
# A struct this size or larger has members with 64-bit offsets. nbctfmerge of the pinned tree
# writes no such struct for a kernel, and the threshold is checked by the parse ending at the
# string table.
LSTRUCT_THRESH = 1 << 29


def section(path, name):
    """The bytes of one section of a 64-bit little-endian ELF file."""
    with open(path, 'rb') as f:
        data = f.read()
    if data[:4] != b'\x7fELF' or data[4] != 2 or data[5] != 1:
        sys.exit('%s: not a 64-bit little-endian ELF file' % path)
    shoff, = struct.unpack_from('<Q', data, 0x28)
    shentsize, shnum, shstrndx = struct.unpack_from('<HHH', data, 0x3a)

    def header(i):
        return struct.unpack_from('<IIQQQQIIQQ', data, shoff + i * shentsize)

    names = header(shstrndx)
    for i in range(shnum):
        h = header(i)
        start = names[4] + h[0]
        if data[start:data.index(b'\0', start)].decode() == name:
            return data[h[4]:h[4] + h[5]]
    sys.exit('%s: no %s section' % (path, name))


class Ctf:
    def __init__(self, raw):
        magic, version, flags = struct.unpack_from('<HBB', raw, 0)
        if magic != 0xcff1 or version != 3:
            sys.exit('not CTF version 3: magic %#x version %d' % (magic, version))
        (_, _, _, self.objtoff, self.funcoff, self.typeoff, self.stroff,
         self.strlen) = struct.unpack_from('<8I', raw, 4)
        self.size = len(raw)
        body = raw[36:]
        if flags & 1:
            body = zlib.decompress(body)
        self.body = body
        self.strs = body[self.stroff:self.stroff + self.strlen]
        self.types = {}
        self.parse()

    def name(self, off):
        if off & 0x80000000:
            return '<ext %d>' % (off & 0x7fffffff)
        return self.strs[off:self.strs.index(b'\0', off)].decode()

    def parse(self):
        body, p, tid = self.body, self.typeoff, 1
        while p < self.stroff:
            name, info, size = struct.unpack_from('<3I', body, p)
            p += 12
            vlen = info & 0xffffff
            if size == 0xffffffff:
                hi, lo = struct.unpack_from('<2I', body, p)
                p += 8
                size = (hi << 32) | lo
            t = {'name': self.name(name), 'kind': KIND.get(info >> 26, 'kind%d' % (info >> 26)),
                 'size': size}
            kind = t['kind']
            if kind in ('int', 'float'):
                enc, = struct.unpack_from('<I', body, p)
                p += 4
                t['enc'] = (enc >> 24, (enc >> 16) & 0xff, enc & 0xffff)
            elif kind == 'array':
                t['arr'] = struct.unpack_from('<3I', body, p)
                p += 12
            elif kind == 'func':
                t['args'] = list(struct.unpack_from('<%dI' % vlen, body, p))
                p += 4 * vlen
            elif kind in ('struct', 'union'):
                members = []
                for _ in range(vlen):
                    if size >= LSTRUCT_THRESH:
                        mname, mtype, hi, lo = struct.unpack_from('<4I', body, p)
                        p += 16
                        members.append((self.name(mname), mtype, (hi << 32) | lo))
                    else:
                        mname, mtype, off = struct.unpack_from('<3I', body, p)
                        p += 12
                        members.append((self.name(mname), mtype, off))
                t['members'] = members
            elif kind == 'enum':
                values = []
                for _ in range(vlen):
                    ename, value = struct.unpack_from('<Ii', body, p)
                    p += 8
                    values.append((self.name(ename), value))
                t['values'] = values
            self.types[tid] = t
            tid += 1
        if p != self.stroff:
            sys.exit('the type section ends at %d, not at the string table at %d' % (p, self.stroff))

    def ref(self, i, depth=0):
        """How a use of type i is written."""
        if i == 0:
            return 'void'
        t = self.types.get(i)
        if t is None:
            return '<no type %d>' % i
        kind = t['kind']
        if kind in ('int', 'float'):
            enc, off, bits = t['enc']
            if off == 0 and bits == 8 * t['size']:
                return t['name']
            return '%s:%d@%d' % (t['name'], bits, off)
        if kind in ('struct', 'union', 'enum', 'forward'):
            if t['name']:
                return '%s %s' % (kind, t['name'])
            if depth > 3:
                return 'anonymous %s' % kind
            return self.full(i, depth + 1)
        if kind == 'typedef':
            return t['name']
        if kind == 'ptr':
            return self.ref(t['size'], depth) + ' *'
        if kind in ('const', 'volatile', 'restrict'):
            return '%s %s' % (kind, self.ref(t['size'], depth))
        if kind == 'array':
            return '%s[%d]' % (self.ref(t['arr'][0], depth), t['arr'][2])
        if kind == 'func':
            args = ', '.join(self.ref(a, depth) for a in t['args'])
            return '%s (*)(%s)' % (self.ref(t['size'], depth), args)
        return '<%s>' % kind

    def full(self, i, depth=0):
        """Type i with its members or values."""
        t = self.types[i]
        kind = t['kind']
        if kind in ('struct', 'union'):
            members = '; '.join('%s %s @%d' % (self.ref(mt, depth), mn, mo)
                                for mn, mt, mo in t['members'])
            return '%s %s size %d { %s }' % (kind, t['name'] or '(anonymous)', t['size'], members)
        if kind == 'enum':
            values = ', '.join('%s=%d' % v for v in t['values'])
            return 'enum %s size %d { %s }' % (t['name'] or '(anonymous)', t['size'], values)
        if kind == 'typedef':
            return 'typedef %s = %s' % (t['name'], self.ref(t['size'], depth))
        if kind in ('int', 'float'):
            return '%s %s size %d encoding %s' % (kind, t['name'], t['size'], t['enc'])
        return self.ref(i, depth)

    def named(self):
        return sorted('%s\t%s' % (t['name'], self.full(i)) for i, t in self.types.items()
                      if t['name'] and t['kind'] not in ('ptr', 'array', 'func', 'const',
                                                         'volatile', 'restrict'))

    def all(self):
        return Counter((t['kind'], self.ref(i)) for i, t in self.types.items())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument('g')
    ap.add_argument('r')
    ap.add_argument('--out')
    a = ap.parse_args()
    g = Ctf(section(a.g, '.SUNW_ctf'))
    r = Ctf(section(a.r, '.SUNW_ctf'))
    gn, rn = g.named(), r.named()
    gset, rset = set(gn), set(rn)
    named_g = sorted(gset - rset)
    named_r = sorted(rset - gset)
    ga, ra = g.all(), r.all()
    only_g, only_r = ga - ra, ra - ga

    print('| | G | R |')
    print('|---|---|---|')
    print('| section bytes | %d | %d |' % (g.size, r.size))
    print('| types | %d | %d |' % (len(g.types), len(r.types)))
    print('| named types | %d | %d |' % (len(gn), len(rn)))
    print('| data object bytes | %d | %d |' % (g.funcoff - g.objtoff, r.funcoff - r.objtoff))
    print('| function bytes | %d | %d |' % (g.typeoff - g.funcoff, r.typeoff - r.funcoff))
    print()
    names = sorted({line.split('\t')[0] for line in named_g + named_r})
    print('Named types that are not the same: %d.' % len(names))
    print()
    for label, only in (('G', only_g), ('R', only_r)):
        kinds = Counter()
        for (kind, _), n in only.items():
            kinds[kind] += n
        detail = ', '.join('%d %s' % (n, k) for k, n in kinds.most_common()) or 'none'
        print('Types only in %s: %d (%s).' % (label, sum(only.values()), detail))
        print()
    if names:
        print('```')
        for n in names[:40]:
            print(n)
        if len(names) > 40:
            print('... and %d more' % (len(names) - 40))
        print('```')

    if a.out:
        os.makedirs(a.out, exist_ok=True)
        for name, lines in (('ctf-g.txt', gn), ('ctf-r.txt', rn)):
            with open(os.path.join(a.out, name), 'w') as f:
                f.writelines(line + '\n' for line in lines)
        with open(os.path.join(a.out, 'ctf-named.diff'), 'w') as f:
            f.writelines('< %s\n' % line for line in named_g)
            f.writelines('> %s\n' % line for line in named_r)
        with open(os.path.join(a.out, 'ctf-all.diff'), 'w') as f:
            for (kind, ref), n in sorted(only_g.items()):
                f.write('< %d\t%s\t%s\n' % (n, kind, ref))
            for (kind, ref), n in sorted(only_r.items()):
                f.write('> %d\t%s\t%s\n' % (n, kind, ref))
    return 1 if names or only_g or only_r else 0


if __name__ == '__main__':
    sys.exit(main())
