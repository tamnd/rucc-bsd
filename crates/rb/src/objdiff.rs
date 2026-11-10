//! `rb sections-diff` and `rb symbols-diff`: each object of R's build against the same object of
//! G's (plan 09.5).
//!
//! Objects are paired by path, from the `compile.jsonl` of each build, with each build's directory
//! replaced by `<root>`. Each difference is one line with the unit's source, grouped by kind.
//!
//! `rb sections-diff` compares these:
//! - the section names;
//! - the flags, type, entry size and group of each section both objects have;
//! - the size of each data section;
//! - the entries of each linker set (`set_*` and `link_set_*`). An entry that is a static local
//!   is compared without the `.N` that each compiler appends to its name in its own order.
//!
//! `rb symbols-diff` compares each symbol that is not local: its name, binding, type and
//! visibility, and the size of a data symbol.
//!
//! Some differences are allowed: code and literal sections, debug sections, `.SUNW_ctf`,
//! `.comment`, function sizes and the names a compiler makes up, such as `*.cold` and
//! `*.isra.0`. They are listed but not counted. ctfconvert adds `.SUNW_ctf` after the compile,
//! so an object that the replay compiles again has none. The gate is zero counted differences.

use crate::census::{absolute, is_object};
use harness_shim::record::CompileRecord;
use object::elf;
use object::read::elf::{ElfFile, FileHeader, SectionHeader as _, Sym as _};
use object::{
    Endianness, FileKind, Object as _, ObjectComdat as _, ObjectSection as _, ObjectSymbol as _,
};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::Path;

/// One section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The name.
    pub name: String,
    /// `sh_type`.
    pub kind: u32,
    /// `sh_flags`.
    pub flags: u64,
    /// `sh_entsize`.
    pub entsize: u64,
    /// `sh_size`.
    pub size: u64,
    /// The signature of the group it is in, or empty.
    pub group: String,
}

/// One named symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    /// The name.
    pub name: String,
    /// `STB_*`.
    pub binding: u8,
    /// `STT_*`.
    pub kind: u8,
    /// `STV_*`.
    pub visibility: u8,
    /// `st_size`.
    pub size: u64,
    /// The section it is defined in, or empty.
    pub section: String,
    /// Whether the object defines it.
    pub defined: bool,
}

/// What one object holds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Object {
    /// Every section but the null one, in file order.
    pub sections: Vec<Section>,
    /// Every named symbol but section and file symbols, in table order.
    pub symbols: Vec<Symbol>,
    /// The signatures of the groups.
    pub groups: BTreeSet<String>,
}

/// Read an ELF object.
pub fn read(data: &[u8]) -> Result<Object, String> {
    match FileKind::parse(data).map_err(|e| e.to_string())? {
        FileKind::Elf32 => read_elf::<elf::FileHeader32<Endianness>>(data),
        FileKind::Elf64 => read_elf::<elf::FileHeader64<Endianness>>(data),
        other => Err(format!("not an ELF object but {other:?}")),
    }
}

fn read_elf<Elf: FileHeader<Endian = Endianness>>(data: &[u8]) -> Result<Object, String> {
    let file = ElfFile::<Elf>::parse(data).map_err(|e| e.to_string())?;
    let endian = file.endian();
    let mut group_of = BTreeMap::new();
    let mut groups = BTreeSet::new();
    for comdat in file.comdats() {
        let name = comdat.name().unwrap_or_default().to_string();
        for index in comdat.sections() {
            group_of.insert(index.0, name.clone());
        }
        groups.insert(name);
    }
    let mut names = BTreeMap::new();
    let mut sections = Vec::new();
    for section in file.sections() {
        let header = section.elf_section_header();
        let name = section.name().unwrap_or_default().to_string();
        names.insert(section.index().0, name.clone());
        let kind = header.sh_type(endian);
        if kind == elf::SHT_NULL {
            continue;
        }
        sections.push(Section {
            kind: kind.0,
            flags: header.sh_flags(endian).0,
            entsize: header.sh_entsize(endian).into(),
            size: header.sh_size(endian).into(),
            group: group_of
                .get(&section.index().0)
                .cloned()
                .unwrap_or_default(),
            name,
        });
    }
    let mut symbols = Vec::new();
    for symbol in file.symbols() {
        let sym = symbol.elf_symbol();
        let kind = sym.st_type();
        if kind == elf::STT_SECTION || kind == elf::STT_FILE {
            continue;
        }
        let name = symbol.name().unwrap_or_default();
        if name.is_empty() {
            continue;
        }
        symbols.push(Symbol {
            name: name.to_string(),
            binding: sym.st_bind().0,
            kind: kind.0,
            visibility: sym.st_visibility().0,
            size: symbol.size(),
            section: symbol
                .section_index()
                .and_then(|i| names.get(&i.0).cloned())
                .unwrap_or_default(),
            defined: !symbol.is_undefined(),
        });
    }
    Ok(Object {
        sections,
        symbols,
        groups,
    })
}

/// The `readelf` letters of section flags.
#[must_use]
pub fn flag_letters(flags: u64) -> String {
    let letters: String = [
        (0x1, 'W'),
        (0x2, 'A'),
        (0x4, 'X'),
        (0x10, 'M'),
        (0x20, 'S'),
        (0x40, 'I'),
        (0x80, 'L'),
        (0x100, 'O'),
        (0x200, 'G'),
        (0x400, 'T'),
        (0x800, 'C'),
        (0x20_0000, 'R'),
        (0x8000_0000, 'E'),
    ]
    .iter()
    .filter(|(bit, _)| flags & bit != 0)
    .map(|(_, letter)| *letter)
    .collect();
    if letters.is_empty() {
        "none".to_string()
    } else {
        letters
    }
}

/// The name of a value from a table, or the number.
fn named<T: Copy + Into<u32>>(value: T, table: &[(u32, &str)]) -> String {
    let n: u32 = value.into();
    table
        .iter()
        .find(|(v, _)| *v == n)
        .map_or_else(|| format!("{n:#x}"), |(_, name)| (*name).to_string())
}

fn type_name(kind: u32) -> String {
    named(
        kind,
        &[
            (1, "progbits"),
            (4, "rela"),
            (7, "note"),
            (8, "nobits"),
            (9, "rel"),
            (14, "init_array"),
            (15, "fini_array"),
            (16, "preinit_array"),
            (17, "group"),
            (0x6fff_fff5, "gnu_attributes"),
            (0x7000_0001, "x86_64_unwind"),
        ],
    )
}

fn binding_name(binding: u8) -> String {
    named(
        binding,
        &[(0, "local"), (1, "global"), (2, "weak"), (10, "gnu_unique")],
    )
}

fn symbol_type_name(kind: u8) -> String {
    named(
        kind,
        &[
            (0, "notype"),
            (1, "object"),
            (2, "func"),
            (5, "common"),
            (6, "tls"),
            (10, "gnu_ifunc"),
        ],
    )
}

fn visibility_name(visibility: u8) -> String {
    named(
        visibility,
        &[
            (0, "default"),
            (1, "internal"),
            (2, "hidden"),
            (3, "protected"),
        ],
    )
}

/// Whether a section is a linker set.
#[must_use]
pub fn is_linker_set(name: &str) -> bool {
    name.starts_with("set_") || name.starts_with("link_set_")
}

/// Why a section may be in one build only, or a different size, or `None` when it may not.
#[must_use]
pub fn free_section(section: &Section) -> Option<&'static str> {
    let name = section.name.as_str();
    if section.kind == elf::SHT_REL.0 || section.kind == elf::SHT_RELA.0 {
        Some("relocations follow their section")
    } else if name == ".text" || name.starts_with(".text.") {
        Some("code")
    } else if name == ".rodata" || name.starts_with(".rodata.") {
        Some("read-only data and literals")
    } else if name.starts_with(".debug") || name.starts_with(".zdebug") {
        Some("debug information, which rb ctf-diff compares")
    } else if name == ".SUNW_ctf" {
        Some("CTF, which ctfconvert adds after the compile and rb ctf-diff compares")
    } else if name == ".comment" {
        Some("the compiler's identification")
    } else {
        None
    }
}

/// Why a section's size may differ when the section itself may not be missing, or `None`.
fn free_size(section: &Section) -> Option<&'static str> {
    if let Some(why) = free_section(section) {
        return Some(why);
    }
    let alloc = section.flags & 0x2 != 0;
    let write = section.flags & 0x1 != 0;
    let note = section.kind == elf::SHT_NOTE.0;
    if section.name == ".note.GNU-stack" {
        Some("a marker only")
    } else if is_linker_set(&section.name) || (alloc && write) || note {
        None
    } else if section.name.starts_with(".eh_frame") {
        Some("unwind tables follow the code")
    } else {
        Some("not a data section")
    }
}

/// Why a symbol may be in one build only, or `None` when it may not.
#[must_use]
pub fn free_symbol(name: &str) -> Option<&'static str> {
    let made = name.starts_with(".L")
        || name.starts_with("__x86.get_pc_thunk")
        || name == "_GLOBAL_OFFSET_TABLE_"
        || [".cold", ".isra.", ".constprop.", ".part.", ".lto_priv."]
            .iter()
            .any(|s| name.contains(s));
    made.then_some("a name the compiler makes")
}

/// One difference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    /// What differs, such as `section flags`.
    pub kind: &'static str,
    /// Why it is allowed, or `None` when it counts.
    pub allowed: Option<&'static str>,
    /// The difference, in words.
    pub what: String,
}

fn finding(kind: &'static str, allowed: Option<&'static str>, what: String) -> Finding {
    Finding {
        kind,
        allowed,
        what,
    }
}

/// The differences between the sections of two objects, G first.
#[must_use]
pub fn sections(g: &Object, r: &Object) -> Vec<Finding> {
    let index = |o: &Object| -> BTreeMap<String, Section> {
        let mut out = BTreeMap::new();
        for s in &o.sections {
            let skip = [
                elf::SHT_GROUP,
                elf::SHT_SYMTAB,
                elf::SHT_STRTAB,
                elf::SHT_SYMTAB_SHNDX,
            ];
            if !skip.iter().any(|k| k.0 == s.kind) {
                out.entry(s.name.clone()).or_insert_with(|| s.clone());
            }
        }
        out
    };
    let (gs, rs) = (index(g), index(r));
    let mut out = Vec::new();
    for (name, s) in &gs {
        if !rs.contains_key(name) {
            out.push(finding("section only in G", free_section(s), name.clone()));
        }
    }
    for (name, s) in &rs {
        if !gs.contains_key(name) {
            out.push(finding("section only in R", free_section(s), name.clone()));
        }
    }
    for (name, a) in &gs {
        let Some(b) = rs.get(name) else { continue };
        let attributes = if name == ".comment" || name.starts_with(".debug") {
            free_section(a)
        } else {
            None
        };
        if a.flags != b.flags {
            let what = format!(
                "{name}: {} in G, {} in R",
                flag_letters(a.flags),
                flag_letters(b.flags)
            );
            out.push(finding("section flags", attributes, what));
        }
        if a.kind != b.kind {
            let what = format!(
                "{name}: {} in G, {} in R",
                type_name(a.kind),
                type_name(b.kind)
            );
            out.push(finding("section type", attributes, what));
        }
        if a.entsize != b.entsize {
            let what = format!("{name}: {} in G, {} in R", a.entsize, b.entsize);
            out.push(finding("section entry size", attributes, what));
        }
        if a.group != b.group {
            let what = format!("{name}: {:?} in G, {:?} in R", a.group, b.group);
            out.push(finding("section group", attributes, what));
        }
        if a.size != b.size {
            let what = format!("{name}: {} bytes in G, {} in R", a.size, b.size);
            out.push(finding("section size", free_size(a), what));
        }
    }
    for name in g.groups.difference(&r.groups) {
        out.push(finding("group only in G", free_symbol(name), name.clone()));
    }
    for name in r.groups.difference(&g.groups) {
        out.push(finding("group only in R", free_symbol(name), name.clone()));
    }
    let (gsets, rsets) = (linker_sets(g), linker_sets(r));
    let empty = BTreeMap::new();
    for set in gsets.keys().chain(rsets.keys()).collect::<BTreeSet<_>>() {
        let (a, b) = (
            gsets.get(set).unwrap_or(&empty),
            rsets.get(set).unwrap_or(&empty),
        );
        for (kind, mine, theirs) in [
            ("linker set entry only in G", a, b),
            ("linker set entry only in R", b, a),
        ] {
            for (entry, &here) in mine {
                let there = theirs.get(entry).copied().unwrap_or(0);
                for _ in there..here {
                    out.push(finding(kind, None, format!("{set}: {entry}")));
                }
            }
        }
    }
    out
}

/// The symbols each linker set section of an object defines, by set, with the number of entries
/// of each name. The name of a local symbol loses the `.N` of [`unnumbered`].
#[must_use]
pub fn linker_sets(o: &Object) -> BTreeMap<String, BTreeMap<String, usize>> {
    let mut out: BTreeMap<String, BTreeMap<String, usize>> = BTreeMap::new();
    for s in &o.sections {
        if is_linker_set(&s.name) {
            out.entry(s.name.clone()).or_default();
        }
    }
    for sym in &o.symbols {
        if sym.defined && is_linker_set(&sym.section) {
            let name = if sym.binding == elf::STB_LOCAL.0 {
                unnumbered(&sym.name)
            } else {
                &sym.name
            };
            *out.entry(sym.section.clone())
                .or_default()
                .entry(name.to_string())
                .or_default() += 1;
        }
    }
    out
}

/// A name without the `.N` that GCC and rucc append to a static local, such as the
/// `__set_sysctl_set_sym_sysctl___debug_fail_point_x.4` of a fail point in a function. Each
/// compiler gives the numbers in its own order, so they are not compared.
#[must_use]
pub fn unnumbered(name: &str) -> &str {
    match name.rsplit_once('.') {
        Some((stem, n))
            if !stem.is_empty() && !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()) =>
        {
            stem
        }
        _ => name,
    }
}

/// The differences between the symbols of two objects that are not local, G first.
#[must_use]
pub fn symbols(g: &Object, r: &Object) -> Vec<Finding> {
    let index = |o: &Object| -> BTreeMap<String, Symbol> {
        let mut out = BTreeMap::new();
        for s in &o.symbols {
            if s.binding != elf::STB_LOCAL.0 {
                out.entry(s.name.clone()).or_insert_with(|| s.clone());
            }
        }
        out
    };
    let (gs, rs) = (index(g), index(r));
    let mut out = Vec::new();
    for (side, mine, theirs) in [("G", &gs, &rs), ("R", &rs, &gs)] {
        for (name, s) in mine {
            if theirs.contains_key(name) {
                continue;
            }
            let kind = match (side, s.defined) {
                ("G", true) => "symbol only in G",
                ("G", false) => "undefined reference only in G",
                (_, true) => "symbol only in R",
                (_, false) => "undefined reference only in R",
            };
            out.push(finding(kind, free_symbol(name), name.clone()));
        }
    }
    for (name, a) in &gs {
        let Some(b) = rs.get(name) else { continue };
        let free = free_symbol(name);
        let both = |g: String, r: String| format!("{name}: {g} in G, {r} in R");
        if a.defined != b.defined {
            let say = |d: bool| if d { "defined" } else { "undefined" }.to_string();
            out.push(finding(
                "symbol defined in one build",
                free,
                both(say(a.defined), say(b.defined)),
            ));
            continue;
        }
        if a.binding != b.binding {
            out.push(finding(
                "symbol binding",
                free,
                both(binding_name(a.binding), binding_name(b.binding)),
            ));
        }
        if a.visibility != b.visibility {
            out.push(finding(
                "symbol visibility",
                free,
                both(visibility_name(a.visibility), visibility_name(b.visibility)),
            ));
        }
        if !a.defined {
            continue;
        }
        if a.kind != b.kind {
            out.push(finding(
                "symbol type",
                free,
                both(symbol_type_name(a.kind), symbol_type_name(b.kind)),
            ));
        }
        if a.size != b.size {
            let code = a.kind == elf::STT_FUNC.0 || a.kind == elf::STT_GNU_IFUNC.0;
            let why = if code {
                Some("function sizes are free")
            } else {
                free
            };
            out.push(finding(
                "symbol size",
                why,
                both(a.size.to_string(), b.size.to_string()),
            ));
        }
    }
    out
}

/// Which comparison to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// `rb sections-diff`.
    Sections,
    /// `rb symbols-diff`.
    Symbols,
}

impl Mode {
    /// The command's name.
    #[must_use]
    pub fn command(self) -> &'static str {
        match self {
            Self::Sections => "sections-diff",
            Self::Symbols => "symbols-diff",
        }
    }
}

/// One object of a build.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unit {
    /// The object's path on disk.
    pub path: String,
    /// The source, absolute, with the root replaced, or empty.
    pub source: String,
}

/// The objects of a build, keyed by path with `root` replaced by `<root>`. When two calls made
/// the same object, the last one counts.
#[must_use]
pub fn units(records: &[CompileRecord], root: &str) -> BTreeMap<String, Unit> {
    let mut out = BTreeMap::new();
    let rooted = |path: String| {
        if root.is_empty() {
            path
        } else {
            path.replace(root, "<root>")
        }
    };
    for r in records {
        let slot = r.slot.as_deref().unwrap_or("cc");
        if !r.is_unit() || !r.succeeded() || !matches!(slot, "cc" | "c++") {
            continue;
        }
        let Some(output) = r
            .outputs
            .iter()
            .map(|o| o.path.as_str())
            .find(|p| is_object(p))
        else {
            continue;
        };
        let source = r
            .inputs
            .iter()
            .map(|i| i.path.as_str())
            .find(|p| !is_object(p) && Path::new(p).extension().is_none_or(|e| e != "a"))
            .map(|p| rooted(absolute(&r.cwd, p)))
            .unwrap_or_default();
        let path = absolute(&r.cwd, output);
        out.insert(rooted(path.clone()), Unit { path, source });
    }
    out
}

/// What one pair of objects gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pair {
    /// The differences.
    Compared(Vec<Finding>),
    /// One of the files is gone, as when the build removed a temporary object.
    Gone,
    /// One of the files cannot be read, with why.
    Unreadable(String),
}

/// Compare one pair of files.
#[must_use]
pub fn compare_files(mode: Mode, g: &str, r: &str) -> Pair {
    let load = |path: &str| match std::fs::read(path) {
        Ok(data) => read(&data).map(Some).map_err(|e| format!("{path}: {e}")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("{path}: {e}")),
    };
    match (load(g), load(r)) {
        (Err(e), _) | (_, Err(e)) => Pair::Unreadable(e),
        (Ok(Some(a)), Ok(Some(b))) => Pair::Compared(match mode {
            Mode::Sections => sections(&a, &b),
            Mode::Symbols => symbols(&a, &b),
        }),
        _ => Pair::Gone,
    }
}

/// A kind of difference and why it is allowed, or `None` when it counts.
pub type Kind = (&'static str, Option<&'static str>);

/// The comparison of two builds.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ObjDiff {
    /// Objects both builds made.
    pub paired: usize,
    /// Paired objects that one build no longer has on disk.
    pub gone: usize,
    /// Objects only G made.
    pub only_g: Vec<String>,
    /// Objects only R made.
    pub only_r: Vec<String>,
    /// Objects that could not be read, which count.
    pub unreadable: Vec<String>,
    /// The differences by kind and reason, each with the unit's source and the difference.
    pub found: BTreeMap<Kind, Vec<(String, String)>>,
}

impl ObjDiff {
    /// The differences that count.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.unreadable.len()
            + self
                .found
                .iter()
                .filter(|((_, why), _)| why.is_none())
                .map(|(_, v)| v.len())
                .sum::<usize>()
    }

    /// The report. At most `limit` lines of each list are printed.
    #[must_use]
    pub fn report(&self, mode: Mode, limit: usize) -> String {
        let mut s = String::new();
        let allowed: usize = self
            .found
            .iter()
            .filter(|((_, why), _)| why.is_some())
            .map(|(_, v)| v.len())
            .sum();
        let _ = writeln!(
            s,
            "rb {}: objects paired: {}, no longer on disk: {}, only in G: {}, only in R: {}. Counted differences: {}. Allowed differences: {allowed}.",
            mode.command(),
            self.paired,
            self.gone,
            self.only_g.len(),
            self.only_r.len(),
            self.counted(),
        );
        let list = |s: &mut String, title: String, lines: &[(String, String)]| {
            let _ = writeln!(s, "  {title} ({}):", lines.len());
            for (source, what) in lines.iter().take(limit) {
                let _ = writeln!(s, "    {source}: {what}");
            }
            if lines.len() > limit {
                let _ = writeln!(s, "    and {} more", lines.len() - limit);
            }
        };
        if self.counted() > 0 {
            let _ = writeln!(s, "\nCounted differences:");
            if !self.unreadable.is_empty() {
                let lines: Vec<(String, String)> = self
                    .unreadable
                    .iter()
                    .map(|e| ("object".to_string(), e.clone()))
                    .collect();
                list(&mut s, "cannot read".to_string(), &lines);
            }
            for ((kind, _), lines) in self.found.iter().filter(|((_, why), _)| why.is_none()) {
                list(&mut s, (*kind).to_string(), lines);
            }
        }
        if allowed > 0 {
            let _ = writeln!(s, "\nAllowed differences, not counted:");
            for ((kind, why), lines) in &self.found {
                if let Some(why) = why {
                    list(&mut s, format!("{kind}, {why}"), lines);
                }
            }
        }
        for (side, objects) in [("G", &self.only_g), ("R", &self.only_r)] {
            if !objects.is_empty() {
                let _ = writeln!(
                    s,
                    "\nObjects only {side} made, not counted here; rb flags-diff counts them:"
                );
                for o in objects.iter().take(limit) {
                    let _ = writeln!(s, "  {o}");
                }
                if objects.len() > limit {
                    let _ = writeln!(s, "  and {} more", objects.len() - limit);
                }
            }
        }
        s
    }
}

/// Compare the objects of two builds, `jobs` pairs at a time.
#[must_use]
pub fn compare(
    mode: Mode,
    g: &BTreeMap<String, Unit>,
    r: &BTreeMap<String, Unit>,
    jobs: usize,
) -> ObjDiff {
    let mut diff = ObjDiff {
        only_g: g.keys().filter(|k| !r.contains_key(*k)).cloned().collect(),
        only_r: r.keys().filter(|k| !g.contains_key(*k)).cloned().collect(),
        ..ObjDiff::default()
    };
    let pairs: Vec<(&String, &Unit, &Unit)> = g
        .iter()
        .filter_map(|(k, a)| r.get(k).map(|b| (k, a, b)))
        .collect();
    diff.paired = pairs.len();
    let tasks: Vec<harness_agent::pool::Task<'_, Pair>> = pairs
        .iter()
        .map(|(_, a, b)| {
            Box::new(move || compare_files(mode, &a.path, &b.path))
                as harness_agent::pool::Task<'_, _>
        })
        .collect();
    for ((key, a, _), pair) in pairs.iter().zip(harness_agent::pool::run(jobs, tasks)) {
        match pair {
            Pair::Gone => diff.gone += 1,
            Pair::Unreadable(e) => diff.unreadable.push(e),
            Pair::Compared(found) => {
                let source = if a.source.is_empty() {
                    (*key).clone()
                } else {
                    a.source.clone()
                };
                for f in found {
                    diff.found
                        .entry((f.kind, f.allowed))
                        .or_default()
                        .push((source.clone(), f.what));
                }
            }
        }
    }
    diff
}

/// `rb sections-diff` and `rb symbols-diff` on the command line.
pub mod cli {
    use super::{Mode, compare, units};
    use crate::cli::Args;
    use harness_shim::record::read_log;
    use std::path::Path;

    /// Run one of the two commands. Returns the report and whether no difference counted.
    pub fn run(args: &Args, mode: Mode) -> Result<(String, bool), String> {
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err("give --g and --r, the compile.jsonl of each build".into());
        };
        let number = |key: &str, default: usize| -> Result<usize, String> {
            args.get(key).map_or(Ok(default), |n| {
                n.parse()
                    .map_err(|_| format!("--{key} {n} is not a number"))
            })
        };
        let read = |log: &str, root: &str| {
            let (records, _) = read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
            Ok::<_, String>(units(&records, root))
        };
        let g_units = read(g, args.get("g-root").unwrap_or_default())?;
        let r_units = read(r, args.get("r-root").unwrap_or_default())?;
        let diff = compare(
            mode,
            &g_units,
            &r_units,
            number("jobs", harness_agent::pool::default_jobs(1))?,
        );
        let text = diff.report(mode, number("limit", 20)?);
        Ok((text, diff.paired > 0 && diff.counted() == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object as Writer, Symbol as WSymbol, SymbolSection};
    use object::{Architecture, BinaryFormat, SectionKind, SymbolFlags, SymbolKind, SymbolScope};

    /// An x86-64 object: `f` in `.text`, `table` of `size` bytes in `.data`, and an entry in the
    /// linker set `set_sysinit_set`.
    fn object(size: usize, fsize: u64, entry: &str, hidden: bool) -> Vec<u8> {
        let mut obj = Writer::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        obj.append_section_data(text, &vec![0x90; usize::try_from(fsize).unwrap()], 16);
        obj.add_symbol(WSymbol {
            name: b"f".to_vec(),
            value: 0,
            size: fsize,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        let data = obj.add_section(Vec::new(), b".data".to_vec(), SectionKind::Data);
        obj.append_section_data(data, &vec![0; size], 8);
        obj.add_symbol(WSymbol {
            name: b"table".to_vec(),
            value: 0,
            size: size as u64,
            kind: SymbolKind::Data,
            scope: if hidden {
                SymbolScope::Linkage
            } else {
                SymbolScope::Dynamic
            },
            weak: false,
            section: SymbolSection::Section(data),
            flags: SymbolFlags::None,
        });
        let set = obj.add_section(Vec::new(), b"set_sysinit_set".to_vec(), SectionKind::Data);
        obj.append_section_data(set, &[0; 8], 8);
        obj.add_symbol(WSymbol {
            name: entry.as_bytes().to_vec(),
            value: 0,
            size: 8,
            kind: SymbolKind::Data,
            scope: SymbolScope::Compilation,
            weak: false,
            section: SymbolSection::Section(set),
            flags: SymbolFlags::None,
        });
        obj.write().unwrap()
    }

    fn kinds(found: &[Finding]) -> Vec<(&'static str, bool)> {
        found
            .iter()
            .map(|f| (f.kind, f.allowed.is_none()))
            .collect()
    }

    #[test]
    fn an_object_reads_back() {
        let o = read(&object(16, 4, "__set_a", false)).unwrap();
        let data = o.sections.iter().find(|s| s.name == ".data").unwrap();
        assert_eq!((data.size, flag_letters(data.flags).as_str()), (16, "WA"));
        assert_eq!(
            linker_sets(&o)["set_sysinit_set"].keys().next().unwrap(),
            "__set_a"
        );
        let table = o.symbols.iter().find(|s| s.name == "table").unwrap();
        assert_eq!(
            (table.binding, table.kind, table.size),
            (elf::STB_GLOBAL.0, elf::STT_OBJECT.0, 16)
        );
    }

    #[test]
    fn static_locals_in_linker_sets_lose_their_numbers() {
        assert_eq!(unnumbered("__set_x_sym_a.4"), "__set_x_sym_a");
        assert_eq!(unnumbered("__set_x_sym_a"), "__set_x_sym_a");
        assert_eq!(unnumbered("a.b"), "a.b");
        assert_eq!(unnumbered(".4"), ".4");
        let o = |names: &[&str]| Object {
            sections: vec![Section {
                name: "set_x".into(),
                kind: elf::SHT_PROGBITS.0,
                flags: 0x3,
                entsize: 0,
                size: 8 * names.len() as u64,
                group: String::new(),
            }],
            symbols: names
                .iter()
                .map(|n| Symbol {
                    name: (*n).into(),
                    binding: elf::STB_LOCAL.0,
                    kind: elf::STT_OBJECT.0,
                    visibility: 0,
                    size: 8,
                    section: "set_x".into(),
                    defined: true,
                })
                .collect(),
            groups: BTreeSet::new(),
        };
        let g = o(&["__set_x_sym_a.4", "__set_x_sym_a.5", "__set_x_sym_b.3"]);
        let r = o(&["__set_x_sym_a.1", "__set_x_sym_a.3", "__set_x_sym_b.5"]);
        assert!(sections(&g, &r).is_empty());
        let r = o(&["__set_x_sym_a.1", "__set_x_sym_b.5"]);
        let found = sections(&g, &r);
        assert_eq!(
            kinds(&found),
            [("section size", true), ("linker set entry only in G", true)]
        );
        assert_eq!(found[1].what, "set_x: __set_x_sym_a");
    }

    #[test]
    fn code_size_is_free_and_data_size_counts() {
        let g = read(&object(16, 4, "__set_a", false)).unwrap();
        let r = read(&object(16, 9, "__set_a", false)).unwrap();
        assert_eq!(kinds(&sections(&g, &r)), [("section size", false)]);
        assert_eq!(kinds(&symbols(&g, &r)), [("symbol size", false)]);
        let r = read(&object(24, 4, "__set_b", true)).unwrap();
        assert_eq!(
            kinds(&sections(&g, &r)),
            [
                ("section size", true),
                ("linker set entry only in G", true),
                ("linker set entry only in R", true)
            ]
        );
        assert_eq!(
            kinds(&symbols(&g, &r)),
            [("symbol visibility", true), ("symbol size", true)]
        );
    }

    #[test]
    fn compiler_made_names_and_debug_sections_are_allowed() {
        assert!(free_symbol("f.cold").is_some() && free_symbol("g.isra.0").is_some());
        assert!(free_symbol("table").is_none());
        let s = |name: &str, kind: u32, flags: u64| Section {
            name: name.into(),
            kind,
            flags,
            entsize: 0,
            size: 1,
            group: String::new(),
        };
        assert!(free_section(&s(".text.unlikely", 1, 6)).is_some());
        assert!(free_section(&s(".debug_info", 1, 0)).is_some());
        assert!(free_section(&s(".SUNW_ctf", 1, 0)).is_some());
        assert!(free_section(&s(".data", 1, 3)).is_none());
        assert!(free_size(&s(".data", 1, 3)).is_none());
        assert!(free_size(&s("set_x", 1, 2)).is_none());
        assert!(free_size(&s(".eh_frame", 1, 2)).is_some());
    }

    #[test]
    fn two_builds_pair_by_object_and_report_each_kind() {
        let dir = std::env::temp_dir().join(format!("rb-objdiff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for side in ["g", "r"] {
            std::fs::create_dir_all(dir.join(side)).unwrap();
        }
        std::fs::write(dir.join("g/a.o"), object(16, 4, "__set_a", false)).unwrap();
        std::fs::write(dir.join("r/a.o"), object(24, 9, "__set_a", false)).unwrap();
        let unit = |side: &str| {
            let path = dir.join(side).join("a.o").display().to_string();
            let mut m = BTreeMap::new();
            m.insert(
                "<root>/a.o".to_string(),
                Unit {
                    path,
                    source: "<root>/a.c".to_string(),
                },
            );
            m
        };
        let mut g = unit("g");
        g.insert(
            "<root>/gone.o".into(),
            Unit {
                path: dir.join("g/gone.o").display().to_string(),
                source: String::new(),
            },
        );
        g.insert("<root>/b.o".into(), g["<root>/a.o"].clone());
        let mut r = unit("r");
        r.insert(
            "<root>/gone.o".into(),
            Unit {
                path: dir.join("r/gone.o").display().to_string(),
                source: String::new(),
            },
        );
        let diff = compare(Mode::Sections, &g, &r, 2);
        std::fs::remove_dir_all(&dir).ok();
        assert_eq!((diff.paired, diff.gone, diff.only_g.len()), (2, 1, 1));
        assert_eq!(diff.counted(), 1);
        let text = diff.report(Mode::Sections, 10);
        assert!(
            text.contains("<root>/a.c: .data: 16 bytes in G, 24 in R"),
            "{text}"
        );
        assert!(text.contains("section size, code (1)"), "{text}");
        assert!(text.contains("Objects only G made"), "{text}");
    }
}
