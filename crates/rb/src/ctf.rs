//! Reading the CTF of a kernel or a module (plan 09.6), for `rb ctf-diff`.
//!
//! `ctfmerge` writes the CTF into the `.SUNW_ctf` section of the kernel and of each module. The
//! section starts with a header: the magic `0xcff1`, the version, the flags, and eight 32-bit words
//! (the parent's label and name, and the offsets of the label, data object, function, type and
//! string sections, and the length of the string section). With flag 1 the part after the
//! header is zlib data. NetBSD trunk and FreeBSD write version 3, where a type id is 32 bits.
//!
//! The type section is a list of types, with ids from 1. Each type has a name, an info word (the
//! kind in the top 6 bits and the count of members, values or arguments in the low 24 bits) and
//! a size or a type id. A size of `0xffffffff` is followed by a 64-bit size. Then come the kind's
//! own words: an encoding for an integer or a float, the element type, the index type and the
//! count for an array, the argument types for a function, the members of a struct or union, and
//! the values of an enum.
//!
//! The data object section has one type id, and the function section one record, for each symbol
//! of the symbol table that `ctfmerge` does not skip, in the order of the symbol table. A
//! function record is an info word, the return type and the argument types, or a single zero
//! word when the function has no type.
//!
//! A type is written here in a form with no type ids, so that two kernels can be compared: a
//! struct or union with its members, their types and their bit offsets, an enum with its values, a
//! typedef with what it names. A named struct, union or enum is written by its name where it is
//! used, and an anonymous one in full.

use object::elf;
use object::read::elf::{ElfFile64, Sym};
use object::{Endianness, Object, ObjectSection};
use std::collections::BTreeMap;

/// The magic number at the start of the CTF header.
const MAGIC: u16 = 0xcff1;
/// The size of the version 3 header.
const HEADER: usize = 36;
/// A struct this size or larger has members with 64-bit offsets (`CTF_V3_LSTRUCT_THRESH`).
const LSTRUCT_THRESH: u64 = 1 << 29;
/// A size word with this value is followed by a 64-bit size.
const LSIZE_SENT: u32 = 0xffff_ffff;

/// The names of the kinds, by number.
const KINDS: [&str; 14] = [
    "unknown", "int", "float", "ptr", "array", "func", "struct", "union", "enum", "forward",
    "typedef", "volatile", "const", "restrict",
];

/// What a type has after its three words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Detail {
    /// Nothing: a pointer, a typedef, a qualifier or a forward declaration.
    None,
    /// The encoding of an integer or a float: the format, the bit offset and the bits.
    Encoding(u32, u32, u32),
    /// An array: the element type, the index type and the count.
    Array(u32, u32, u32),
    /// A function type: the argument types. A last argument of 0 means `...`.
    Args(Vec<u32>),
    /// A struct or union: the members, as name, type and bit offset.
    Members(Vec<(String, u32, u64)>),
    /// An enum: the values.
    Values(Vec<(String, i32)>),
}

/// One type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Type {
    /// The name, or empty.
    pub name: String,
    /// The kind, an index into the kind names.
    pub kind: usize,
    /// The size in bytes, or the type id this type refers to for a pointer, a typedef, a
    /// qualifier and a function (its return type).
    pub size: u64,
    /// The kind's own words.
    pub detail: Detail,
}

impl Type {
    /// The kind's name, such as `struct`.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        KINDS.get(self.kind).copied().unwrap_or("unknown")
    }
}

/// A function's type: the return type and the argument types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    /// The return type.
    pub ret: u32,
    /// The argument types. A last argument of 0 means `...`.
    pub args: Vec<u32>,
}

/// The CTF of one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Ctf {
    /// The header words after the magic, the version and the flags: the parent's label and
    /// name, the offsets of the label, data object, function, type and string sections, and the
    /// length of the string section.
    pub header: [u32; 8],
    /// The bytes of the section, as stored.
    pub section_bytes: usize,
    /// The bytes of the data object section.
    pub object_bytes: usize,
    /// The bytes of the function section.
    pub function_bytes: usize,
    /// The types, by id.
    pub types: BTreeMap<u32, Type>,
    /// The type of each data object symbol that has one, by symbol name, in symbol table order.
    pub objects: Vec<(String, u32)>,
    /// The type of each function symbol that has one, by symbol name, in symbol table order.
    pub functions: Vec<(String, Function)>,
    /// The data object symbols `ctfmerge` gave no type.
    pub untyped_objects: Vec<String>,
    /// The function symbols `ctfmerge` gave no type.
    pub untyped_functions: Vec<String>,
    /// Whether the data object and function sections had exactly one entry for each symbol
    /// `ctfmerge` writes one for. When they do not, the two lists above are not to be trusted.
    pub symbols_match: bool,
}

/// Reads little-endian words from a byte slice.
struct Words<'a> {
    bytes: &'a [u8],
}

impl Words<'_> {
    fn u32(&self, at: usize) -> Result<u32, String> {
        self.bytes
            .get(at..at + 4)
            .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .ok_or_else(|| format!("the CTF data ends before offset {at}"))
    }
}

/// Whether ELF data has a `.SUNW_ctf` section.
#[must_use]
pub fn has_ctf(data: &[u8]) -> bool {
    ElfFile64::<Endianness>::parse(data).is_ok_and(|f| f.section_by_name(".SUNW_ctf").is_some())
}

/// Read the CTF of a 64-bit ELF file, with the types of its symbols.
pub fn read(data: &[u8]) -> Result<Ctf, String> {
    let file = ElfFile64::<Endianness>::parse(data).map_err(|e| e.to_string())?;
    let section = file
        .section_by_name(".SUNW_ctf")
        .ok_or("no .SUNW_ctf section")?;
    let raw = section.data().map_err(|e| e.to_string())?;
    let (mut ctf, body) = parse_body(raw)?;
    let symbols = symbols(&file)?;
    ctf.symbols_match = symbol_types(&mut ctf, &body, &symbols)?;
    Ok(ctf)
}

/// The header words after the preamble: parent label, parent name, label, object, function,
/// type and string offsets, and the string length.
fn header(raw: &[u8]) -> Result<[u32; 8], String> {
    if raw.len() < HEADER {
        return Err("the CTF section is shorter than its header".into());
    }
    let magic = u16::from_le_bytes([raw[0], raw[1]]);
    if magic != MAGIC || raw[2] != 3 {
        return Err(format!(
            "not CTF version 3: magic {magic:#x} version {}",
            raw[2]
        ));
    }
    let w = Words { bytes: raw };
    let mut h = [0u32; 8];
    for (i, v) in h.iter_mut().enumerate() {
        *v = w.u32(4 + 4 * i)?;
    }
    Ok(h)
}

/// The part after the header, inflated if it is compressed.
fn body(raw: &[u8]) -> Result<Vec<u8>, String> {
    header(raw)?;
    let rest = &raw[HEADER..];
    if raw[3] & 1 == 1 {
        miniz_oxide::inflate::decompress_to_vec_zlib(rest)
            .map_err(|e| format!("the CTF data does not inflate: {e:?}"))
    } else {
        Ok(rest.to_vec())
    }
}

/// Read a CTF section: the header and the types. The symbol lists are left empty.
pub fn parse(raw: &[u8]) -> Result<Ctf, String> {
    parse_body(raw).map(|(c, _)| c)
}

/// [`parse`], with the part after the header, inflated.
fn parse_body(raw: &[u8]) -> Result<(Ctf, Vec<u8>), String> {
    let h = header(raw)?;
    let body = body(raw)?;
    let (objtoff, funcoff, typeoff, stroff, strlen) = (
        h[3] as usize,
        h[4] as usize,
        h[5] as usize,
        h[6] as usize,
        h[7] as usize,
    );
    if !(objtoff <= funcoff && funcoff <= typeoff && typeoff <= stroff)
        || stroff + strlen > body.len()
    {
        return Err("the CTF header's offsets are not in order".into());
    }
    let reader = Reader {
        w: Words { bytes: &body },
        strs: &body[stroff..stroff + strlen],
    };
    let mut types = BTreeMap::new();
    let mut p = typeoff;
    let mut id = 1u32;
    while p < stroff {
        let (t, next) = reader.one_type(p)?;
        types.insert(id, t);
        p = next;
        id += 1;
    }
    if p != stroff {
        return Err(format!(
            "the type section ends at {p}, not at the string table at {stroff}"
        ));
    }
    let ctf = Ctf {
        header: h,
        section_bytes: raw.len(),
        object_bytes: funcoff - objtoff,
        function_bytes: typeoff - funcoff,
        types,
        objects: Vec::new(),
        functions: Vec::new(),
        untyped_objects: Vec::new(),
        untyped_functions: Vec::new(),
        symbols_match: false,
    };
    Ok((ctf, body))
}

/// Reads the types of an inflated CTF body.
struct Reader<'a> {
    w: Words<'a>,
    strs: &'a [u8],
}

impl Reader<'_> {
    /// The string at `off` in the string table.
    fn name(&self, off: u32) -> String {
        if off & 0x8000_0000 != 0 {
            return format!("<ext {}>", off & 0x7fff_ffff);
        }
        let s = self.strs.get(off as usize..).unwrap_or_default();
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        String::from_utf8_lossy(&s[..end]).into_owned()
    }

    /// The type at `p`, and where the next one starts.
    fn one_type(&self, mut p: usize) -> Result<(Type, usize), String> {
        let w = &self.w;
        let (tname, info, size_word) = (w.u32(p)?, w.u32(p + 4)?, w.u32(p + 8)?);
        p += 12;
        let vlen = (info & 0x00ff_ffff) as usize;
        let mut size = u64::from(size_word);
        if size_word == LSIZE_SENT {
            size = (u64::from(w.u32(p)?) << 32) | u64::from(w.u32(p + 4)?);
            p += 8;
        }
        let kind = (info >> 26) as usize;
        let (detail, p) = self.detail(kind, vlen, size, p)?;
        let t = Type {
            name: self.name(tname),
            kind,
            size,
            detail,
        };
        Ok((t, p))
    }

    /// The words of a type of `kind` after its three words, and where they end.
    fn detail(
        &self,
        kind: usize,
        vlen: usize,
        size: u64,
        mut p: usize,
    ) -> Result<(Detail, usize), String> {
        let w = &self.w;
        let detail = match KINDS.get(kind).copied().unwrap_or("unknown") {
            "int" | "float" => {
                let enc = w.u32(p)?;
                p += 4;
                Detail::Encoding(enc >> 24, (enc >> 16) & 0xff, enc & 0xffff)
            }
            "array" => {
                let a = Detail::Array(w.u32(p)?, w.u32(p + 4)?, w.u32(p + 8)?);
                p += 12;
                a
            }
            "func" => {
                let args = (0..vlen)
                    .map(|i| w.u32(p + 4 * i))
                    .collect::<Result<Vec<_>, _>>()?;
                p += 4 * vlen;
                Detail::Args(args)
            }
            "struct" | "union" => {
                let mut members = Vec::with_capacity(vlen);
                for _ in 0..vlen {
                    let (mname, mtype) = (self.name(w.u32(p)?), w.u32(p + 4)?);
                    if size >= LSTRUCT_THRESH {
                        let off = (u64::from(w.u32(p + 8)?) << 32) | u64::from(w.u32(p + 12)?);
                        members.push((mname, mtype, off));
                        p += 16;
                    } else {
                        members.push((mname, mtype, u64::from(w.u32(p + 8)?)));
                        p += 12;
                    }
                }
                Detail::Members(members)
            }
            "enum" => {
                let mut values = Vec::with_capacity(vlen);
                for _ in 0..vlen {
                    let value = i32::from_le_bytes(w.u32(p + 4)?.to_le_bytes());
                    values.push((self.name(w.u32(p)?), value));
                    p += 8;
                }
                Detail::Values(values)
            }
            _ => Detail::None,
        };
        Ok((detail, p))
    }
}

/// One symbol of the symbol table, as `ctfmerge` looks at it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name.
    pub name: String,
    /// Whether the name is empty in the table.
    pub anonymous: bool,
    /// `STT_OBJECT`, `STT_FUNC` or something else.
    pub kind: elf::SymbolType,
    /// Whether it is undefined.
    pub undefined: bool,
    /// Whether it is absolute with the value 0.
    pub absolute_zero: bool,
}

fn symbols(file: &ElfFile64<Endianness>) -> Result<Vec<Symbol>, String> {
    let endian = file.endian();
    let table = file.elf_symbol_table();
    let mut out = Vec::new();
    for sym in table.iter() {
        let name = table
            .symbol_name(endian, sym)
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .map_err(|e| e.to_string())?;
        let shndx = sym.st_shndx(endian);
        out.push(Symbol {
            anonymous: sym.st_name(endian) == 0,
            kind: sym.st_type(),
            undefined: shndx == elf::SHN_UNDEF,
            absolute_zero: shndx == elf::SHN_ABS && sym.st_value(endian) == 0,
            name,
        });
    }
    Ok(out)
}

/// Fill in the types of the symbols from the data object and function sections, as libctf's
/// `init_symtab` reads them. Returns whether both sections were used up exactly.
fn symbol_types(ctf: &mut Ctf, body: &[u8], symbols: &[Symbol]) -> Result<bool, String> {
    let w = Words { bytes: body };
    let (objtoff, funcoff, typeoff) = (
        ctf.header[3] as usize,
        ctf.header[4] as usize,
        ctf.header[5] as usize,
    );
    let mut objt = objtoff;
    let mut func = funcoff;
    for s in symbols {
        if s.anonymous || s.undefined || s.name == "_START_" || s.name == "_END_" {
            continue;
        }
        if s.kind == elf::STT_OBJECT {
            if s.absolute_zero {
                continue;
            }
            if objt >= funcoff {
                ctf.untyped_objects.push(s.name.clone());
                continue;
            }
            let id = w.u32(objt)?;
            objt += 4;
            if id == 0 {
                ctf.untyped_objects.push(s.name.clone());
            } else {
                ctf.objects.push((s.name.clone(), id));
            }
        } else if s.kind == elf::STT_FUNC {
            if func >= typeoff {
                ctf.untyped_functions.push(s.name.clone());
                continue;
            }
            let info = w.u32(func)?;
            let vlen = (info & 0x00ff_ffff) as usize;
            if info >> 26 == 0 && vlen == 0 {
                func += 4;
                ctf.untyped_functions.push(s.name.clone());
                continue;
            }
            let ret = w.u32(func + 4)?;
            let mut args = Vec::with_capacity(vlen);
            for i in 0..vlen {
                args.push(w.u32(func + 8 + 4 * i)?);
            }
            func += 4 * (vlen + 2);
            ctf.functions.push((s.name.clone(), Function { ret, args }));
        }
    }
    Ok(objt == funcoff && func == typeoff)
}

/// How a use of type `id` is written.
#[must_use]
pub fn reference(ctf: &Ctf, id: u32, depth: usize) -> String {
    if id == 0 {
        return "void".into();
    }
    let Some(t) = ctf.types.get(&id) else {
        return format!("<no type {id}>");
    };
    let id_of = |v: u64| u32::try_from(v).unwrap_or(u32::MAX);
    match t.kind_name() {
        "int" | "float" => match t.detail {
            Detail::Encoding(_, off, bits) if off != 0 || u64::from(bits) != 8 * t.size => {
                format!("{}:{bits}@{off}", t.name)
            }
            _ => t.name.clone(),
        },
        k @ ("struct" | "union" | "enum" | "forward") => {
            if !t.name.is_empty() {
                format!("{k} {}", t.name)
            } else if depth > 3 {
                format!("anonymous {k}")
            } else {
                full(ctf, id, depth + 1)
            }
        }
        "typedef" => t.name.clone(),
        "ptr" => format!("{} *", reference(ctf, id_of(t.size), depth)),
        k @ ("const" | "volatile" | "restrict") => {
            format!("{k} {}", reference(ctf, id_of(t.size), depth))
        }
        "array" => match t.detail {
            Detail::Array(elem, _, n) => format!("{}[{n}]", reference(ctf, elem, depth)),
            _ => "<array>".into(),
        },
        "func" => match &t.detail {
            Detail::Args(args) => format!(
                "{} (*)({})",
                reference(ctf, id_of(t.size), depth),
                args_text(ctf, args, depth)
            ),
            _ => "<func>".into(),
        },
        k => format!("<{k}>"),
    }
}

fn args_text(ctf: &Ctf, args: &[u32], depth: usize) -> String {
    args.iter()
        .enumerate()
        .map(|(i, &a)| {
            if a == 0 && i + 1 == args.len() {
                "...".to_string()
            } else {
                reference(ctf, a, depth)
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// Type `id` with its members or values.
#[must_use]
pub fn full(ctf: &Ctf, id: u32, depth: usize) -> String {
    let Some(t) = ctf.types.get(&id) else {
        return format!("<no type {id}>");
    };
    let name = if t.name.is_empty() {
        "(anonymous)"
    } else {
        t.name.as_str()
    };
    match (&t.detail, t.kind_name()) {
        (Detail::Members(members), kind) => {
            let m: Vec<String> = members
                .iter()
                .map(|(mn, mt, mo)| format!("{} {mn} @{mo}", reference(ctf, *mt, depth)))
                .collect();
            format!("{kind} {name} size {} {{ {} }}", t.size, m.join("; "))
        }
        (Detail::Values(values), _) => {
            let v: Vec<String> = values.iter().map(|(n, v)| format!("{n}={v}")).collect();
            format!("enum {name} size {} {{ {} }}", t.size, v.join(", "))
        }
        (_, "typedef") => format!(
            "typedef {} = {}",
            t.name,
            reference(ctf, u32::try_from(t.size).unwrap_or(u32::MAX), depth)
        ),
        (Detail::Encoding(a, b, c), kind) => {
            format!("{kind} {} size {} encoding ({a}, {b}, {c})", t.name, t.size)
        }
        _ => reference(ctf, id, depth),
    }
}

/// A function's type as text: `ret (args)`.
#[must_use]
pub fn function_text(ctf: &Ctf, f: &Function) -> String {
    format!(
        "{} ({})",
        reference(ctf, f.ret, 0),
        args_text(ctf, &f.args, 0)
    )
}

/// The named types that are not pointers, arrays, functions or qualifiers, each as
/// `name<TAB>full form`, sorted.
#[must_use]
pub fn named(ctf: &Ctf) -> Vec<String> {
    let mut out: Vec<String> = ctf
        .types
        .iter()
        .filter(|(_, t)| {
            !t.name.is_empty()
                && !matches!(
                    t.kind_name(),
                    "ptr" | "array" | "func" | "const" | "volatile" | "restrict"
                )
        })
        .map(|(&id, t)| format!("{}\t{}", t.name, full(ctf, id, 0)))
        .collect();
    out.sort();
    out
}

/// Every type as its kind and how a use of it is written, with how many types give that.
#[must_use]
pub fn all(ctf: &Ctf) -> BTreeMap<(String, String), usize> {
    let mut out = BTreeMap::new();
    for (&id, t) in &ctf.types {
        *out.entry((t.kind_name().to_string(), reference(ctf, id, 0)))
            .or_insert(0) += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object as Writer, Symbol as WSymbol, SymbolSection};
    use object::{Architecture, BinaryFormat, SectionKind, SymbolFlags, SymbolKind, SymbolScope};

    fn words(v: &[u32]) -> Vec<u8> {
        v.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// A CTF section with `int`, `struct s { int a; }` and `int (*)(int, ...)`, one data object
    /// of type `struct s` and one function `int f(int)`.
    fn section(compress: bool) -> Vec<u8> {
        let strs = b"\0int\0s\0a\0".to_vec();
        let objects = words(&[2]);
        let functions = words(&[(5 << 26) | 1, 1, 1]);
        let types = words(&[
            1,
            1 << 26,
            4,
            (1 << 24) | 32, // int
            5,
            (6 << 26) | 1,
            4,
            7,
            1,
            0, // struct s { int a @0 }
            0,
            (5 << 26) | 2,
            1,
            1,
            0, // int (*)(int, ...)
        ]);
        let objtoff = 0u32;
        let funcoff = u32::try_from(objects.len()).unwrap();
        let typeoff = funcoff + u32::try_from(functions.len()).unwrap();
        let stroff = typeoff + u32::try_from(types.len()).unwrap();
        let strlen = u32::try_from(strs.len()).unwrap();
        let mut body = objects;
        body.extend(functions);
        body.extend(types);
        body.extend(strs);
        let mut raw = vec![0xf1, 0xcf, 3, u8::from(compress)];
        raw.extend(words(&[0, 0, 0, objtoff, funcoff, typeoff, stroff, strlen]));
        if compress {
            raw.extend(miniz_oxide::deflate::compress_to_vec_zlib(&body, 6));
        } else {
            raw.extend(body);
        }
        raw
    }

    #[test]
    fn types_in_a_plain_and_a_compressed_section() {
        for compress in [false, true] {
            let c = parse(&section(compress)).unwrap();
            assert_eq!(c.types.len(), 3);
            assert_eq!(c.object_bytes, 4);
            assert_eq!(c.function_bytes, 12);
            assert_eq!(full(&c, 2, 0), "struct s size 4 { int a @0 }");
            assert_eq!(reference(&c, 3, 0), "int (*)(int, ...)");
            assert_eq!(
                named(&c),
                vec![
                    "int\tint int size 4 encoding (1, 0, 32)".to_string(),
                    "s\tstruct s size 4 { int a @0 }".to_string()
                ]
            );
        }
    }

    #[test]
    fn not_ctf_version_3() {
        let mut raw = section(false);
        raw[2] = 2;
        assert!(parse(&raw).unwrap_err().contains("not CTF version 3"));
    }

    #[test]
    fn a_type_section_that_does_not_end_at_the_strings() {
        let mut raw = section(false);
        // Make the struct say it has two members, so the parse runs past the string table.
        let at = 36 + 16 + 16 + 4;
        raw[at..at + 4].copy_from_slice(&((6u32 << 26) | 2).to_le_bytes());
        assert!(parse(&raw).is_err());
    }

    /// An object with the CTF section, a function `f` and a data object `table`.
    fn elf(ctf: &[u8]) -> Vec<u8> {
        let mut obj = Writer::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        obj.append_section_data(text, &[0xc3], 16);
        let data = obj.add_section(Vec::new(), b".data".to_vec(), SectionKind::Data);
        obj.append_section_data(data, &[0; 4], 4);
        let c = obj.add_section(Vec::new(), b".SUNW_ctf".to_vec(), SectionKind::Other);
        obj.append_section_data(c, ctf, 4);
        for (name, kind, section) in [
            ("f", SymbolKind::Text, text),
            ("table", SymbolKind::Data, data),
            ("missing", SymbolKind::Data, data),
        ] {
            obj.add_symbol(WSymbol {
                name: name.as_bytes().to_vec(),
                value: 0,
                size: 1,
                kind,
                scope: SymbolScope::Linkage,
                weak: false,
                section: SymbolSection::Section(section),
                flags: SymbolFlags::None,
            });
        }
        obj.write().unwrap()
    }

    #[test]
    fn the_types_of_the_symbols() {
        let data = elf(&section(true));
        assert!(has_ctf(&data));
        let c = read(&data).unwrap();
        assert_eq!(c.objects, vec![("table".to_string(), 2)]);
        assert_eq!(
            c.functions,
            vec![(
                "f".to_string(),
                Function {
                    ret: 1,
                    args: vec![1]
                }
            )]
        );
        assert!(c.symbols_match, "missing comes after the one data object");
        assert_eq!(c.untyped_objects, vec!["missing".to_string()]);
        assert_eq!(function_text(&c, &c.functions[0].1), "int (int)");
    }
}
