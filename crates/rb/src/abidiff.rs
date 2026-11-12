//! `rb abi-diff`: the shared libraries of R's world against those of G's (plan 09.4).
//!
//! The two sides are directories, such as the trees that `scripts/freebsd/libs.sh` writes. Files
//! are paired by their path under the directory. For each shared object, `rb abi-diff` compares:
//! - each dynamic symbol with its version: binding, type, visibility and size;
//! - the version definitions (`.gnu.version_d`) with their parents, and the version needs
//!   (`.gnu.version_r`) with their files;
//! - `DT_SONAME`, the `DT_NEEDED` list, `DT_FLAGS` and `DT_FLAGS_1`;
//! - the program header list, with the flags of each header;
//! - the number of `IRELATIVE` relocations.
//!
//! A symbol is its name and its version: `name@@V` for the default version `V`, `name@V` for a
//! hidden one, such as those of `__sym_compat`, and `name` with no version. For each archive, such
//! as `libc.a`, it compares the `STT_GNU_IFUNC` symbols that each member defines, which a static
//! program resolves through `IRELATIVE`. A link must point to the same place, and a file that is
//! not ELF must have the same bytes.
//!
//! Some differences are allowed: the size of a function, and a symbol that a library uses but does
//! not define. They are listed but not counted. A file on one side only counts. The gate is zero
//! counted differences.

use crate::objdiff::{self, Finding};
use object::elf;
use object::read::elf::{Dyn as _, ElfFile, FileHeader, ProgramHeader as _, Rel as _, Rela as _};
use object::read::elf::{SectionHeader as _, SectionTable, Sym as _};
use object::{Endianness, FileKind};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// One symbol of the dynamic symbol table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DynSymbol {
    /// `STB_*`.
    pub binding: u8,
    /// `STT_*`.
    pub kind: u8,
    /// `STV_*`.
    pub visibility: u8,
    /// `st_size`.
    pub size: u64,
    /// Whether the library defines it.
    pub defined: bool,
}

/// What the dynamic linker sees of one shared object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    /// `DT_SONAME`, or empty.
    pub soname: String,
    /// The `DT_NEEDED` entries, in order.
    pub needed: Vec<String>,
    /// `DT_FLAGS`.
    pub flags: u64,
    /// `DT_FLAGS_1`.
    pub flags_1: u64,
    /// Each program header as its type and flags, in order.
    pub segments: Vec<String>,
    /// Each version definition, as `NAME` or `NAME: PARENT ...`, with `base` before the name of
    /// the base definition.
    pub verdefs: BTreeSet<String>,
    /// Each version need, as `FILE NAME`.
    pub verneeds: BTreeSet<String>,
    /// The dynamic symbols but the null one, by name and version.
    pub symbols: BTreeMap<String, DynSymbol>,
    /// The number of `IRELATIVE` relocations.
    pub irelative: usize,
}

/// Read a shared object.
pub fn read(data: &[u8]) -> Result<Library, String> {
    match FileKind::parse(data).map_err(|e| e.to_string())? {
        FileKind::Elf32 => read_elf::<elf::FileHeader32<Endianness>>(data),
        FileKind::Elf64 => read_elf::<elf::FileHeader64<Endianness>>(data),
        other => Err(format!("not an ELF file but {other:?}")),
    }
}

fn read_elf<Elf: FileHeader<Endian = Endianness>>(data: &[u8]) -> Result<Library, String> {
    let bad = |e: object::Error| e.to_string();
    let file = ElfFile::<Elf>::parse(data).map_err(bad)?;
    let endian = file.endian();
    let header = file.elf_header();
    let sections = file.elf_section_table();
    let mut lib = Library::default();

    if let Some((entries, link)) = sections.dynamic(endian, data).map_err(bad)? {
        let strings = sections.strings(endian, data, link).map_err(bad)?;
        for d in entries {
            let tag = d.d_tag(endian);
            let value: u64 = d.d_val(endian).into();
            let string = || {
                u32::try_from(value)
                    .ok()
                    .and_then(|v| strings.get(v).ok())
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .unwrap_or_default()
            };
            if tag == elf::DT_SONAME {
                lib.soname = string();
            } else if tag == elf::DT_NEEDED {
                lib.needed.push(string());
            } else if tag == elf::DT_FLAGS {
                lib.flags = value;
            } else if tag == elf::DT_FLAGS_1 {
                lib.flags_1 = value;
            }
        }
    }

    for p in header.program_headers(endian, data).map_err(bad)? {
        let flags = p.p_flags(endian).0;
        let letter = |bit: u32, c: char| if flags & bit != 0 { c } else { '-' };
        lib.segments.push(format!(
            "{} {}{}{}",
            segment_name(p.p_type(endian).0),
            letter(elf::PF_R.0, 'r'),
            letter(elf::PF_W.0, 'w'),
            letter(elf::PF_X.0, 'x'),
        ));
    }

    read_versions(sections, endian, data, &mut lib)?;
    read_symbols(sections, endian, data, &mut lib)?;
    lib.irelative = count_irelative(sections, endian, data, header.e_machine(endian))?;
    Ok(lib)
}

/// The version definitions and needs of a shared object.
fn read_versions<Elf: FileHeader<Endian = Endianness>>(
    sections: &SectionTable<'_, Elf>,
    endian: Endianness,
    data: &[u8],
    lib: &mut Library,
) -> Result<(), String> {
    let bad = |e: object::Error| e.to_string();
    if let Some((mut defs, link)) = sections.gnu_verdef(endian, data).map_err(bad)? {
        let strings = sections.strings(endian, data, link).map_err(bad)?;
        while let Some((def, mut auxs)) = defs.next().map_err(bad)? {
            let mut names = Vec::new();
            while let Some(aux) = auxs.next().map_err(bad)? {
                names.push(
                    String::from_utf8_lossy(aux.name(endian, strings).map_err(bad)?).into_owned(),
                );
            }
            let base = def.vd_flags.get(endian).contains(elf::VER_FLG_BASE);
            let mut line = if base {
                "base ".to_string()
            } else {
                String::new()
            };
            line.push_str(names.first().map_or("", String::as_str));
            if names.len() > 1 {
                line.push(':');
                for parent in &names[1..] {
                    line.push(' ');
                    line.push_str(parent);
                }
            }
            lib.verdefs.insert(line);
        }
    }
    if let Some((mut needs, link)) = sections.gnu_verneed(endian, data).map_err(bad)? {
        let strings = sections.strings(endian, data, link).map_err(bad)?;
        while let Some((need, mut auxs)) = needs.next().map_err(bad)? {
            let file =
                String::from_utf8_lossy(need.file(endian, strings).map_err(bad)?).into_owned();
            while let Some(aux) = auxs.next().map_err(bad)? {
                let name = String::from_utf8_lossy(aux.name(endian, strings).map_err(bad)?);
                lib.verneeds.insert(format!("{file} {name}"));
            }
        }
    }

    Ok(())
}

/// The dynamic symbols of a shared object, by name and version.
fn read_symbols<Elf: FileHeader<Endian = Endianness>>(
    sections: &SectionTable<'_, Elf>,
    endian: Endianness,
    data: &[u8],
    lib: &mut Library,
) -> Result<(), String> {
    let bad = |e: object::Error| e.to_string();
    let versions = sections.versions(endian, data).map_err(bad)?;
    let table = sections
        .symbols(endian, data, elf::SHT_DYNSYM)
        .map_err(bad)?;
    for (index, sym) in table.enumerate() {
        if index.0 == 0 {
            continue;
        }
        let name = String::from_utf8_lossy(sym.name(endian, table.strings()).map_err(bad)?);
        if name.is_empty() {
            continue;
        }
        let defined = !sym.is_undefined(endian);
        let mut key = name.into_owned();
        if let Some(versions) = &versions {
            let v = versions.version_index(endian, index);
            if let Some(version) = versions.version(v.index()).map_err(bad)? {
                let version = String::from_utf8_lossy(version.name());
                let at = if defined && !v.is_hidden() { "@@" } else { "@" };
                let _ = write!(key, "{at}{version}");
            }
        }
        lib.symbols.insert(
            key,
            DynSymbol {
                binding: sym.st_bind().0,
                kind: sym.st_type().0,
                visibility: sym.st_visibility().0,
                size: sym.st_size(endian).into(),
                defined,
            },
        );
    }

    Ok(())
}

/// The number of `IRELATIVE` relocations in the alloc sections of a shared object.
fn count_irelative<Elf: FileHeader<Endian = Endianness>>(
    sections: &SectionTable<'_, Elf>,
    endian: Endianness,
    data: &[u8],
    machine: elf::Machine,
) -> Result<usize, String> {
    let bad = |e: object::Error| e.to_string();
    let mut count = 0;
    let irelative = if machine == elf::EM_386 {
        elf::R_386_IRELATIVE.0
    } else {
        elf::R_X86_64_IRELATIVE.0
    };
    for section in sections.iter() {
        if section.sh_flags(endian).0 & elf::SHF_ALLOC.0 == 0 {
            continue;
        }
        if let Some((relas, _)) = section.rela(endian, data).map_err(bad)? {
            count += relas
                .iter()
                .filter(|r| r.r_type(endian, false).0 == irelative)
                .count();
        }
        if let Some((rels, _)) = section.rel(endian, data).map_err(bad)? {
            count += rels
                .iter()
                .filter(|r| r.r_type(endian).0 == irelative)
                .count();
        }
    }
    Ok(count)
}

/// The name of a program header type, as `readelf -l` gives it.
fn segment_name(kind: u32) -> String {
    match kind {
        1 => "LOAD".into(),
        2 => "DYNAMIC".into(),
        3 => "INTERP".into(),
        4 => "NOTE".into(),
        6 => "PHDR".into(),
        7 => "TLS".into(),
        0x6474_e550 => "GNU_EH_FRAME".into(),
        0x6474_e551 => "GNU_STACK".into(),
        0x6474_e552 => "GNU_RELRO".into(),
        0x6474_e553 => "GNU_PROPERTY".into(),
        _ => format!("{kind:#x}"),
    }
}

fn finding(kind: &'static str, allowed: Option<&'static str>, what: String) -> Finding {
    Finding {
        kind,
        allowed,
        what,
    }
}

fn kind_name(kind: u8) -> String {
    match kind {
        0 => "NOTYPE".into(),
        1 => "OBJECT".into(),
        2 => "FUNC".into(),
        5 => "COMMON".into(),
        6 => "TLS".into(),
        10 => "IFUNC".into(),
        _ => kind.to_string(),
    }
}

fn binding_name(binding: u8) -> String {
    match binding {
        0 => "LOCAL".into(),
        1 => "GLOBAL".into(),
        2 => "WEAK".into(),
        10 => "UNIQUE".into(),
        _ => binding.to_string(),
    }
}

fn visibility_name(visibility: u8) -> String {
    match visibility {
        0 => "DEFAULT".into(),
        1 => "INTERNAL".into(),
        2 => "HIDDEN".into(),
        3 => "PROTECTED".into(),
        _ => visibility.to_string(),
    }
}

fn is_function(kind: u8) -> bool {
    kind == elf::STT_FUNC.0 || kind == elf::STT_GNU_IFUNC.0
}

/// The differences between two shared objects, G first.
#[must_use]
pub fn libraries(g: &Library, r: &Library) -> Vec<Finding> {
    let mut found = Vec::new();
    if g.soname != r.soname {
        found.push(finding(
            "DT_SONAME",
            None,
            format!("G {:?}, R {:?}", g.soname, r.soname),
        ));
    }
    if g.needed != r.needed {
        found.push(finding(
            "DT_NEEDED",
            None,
            format!("G {}, R {}", g.needed.join(" "), r.needed.join(" ")),
        ));
    }
    if g.flags != r.flags {
        found.push(finding(
            "DT_FLAGS",
            None,
            format!("G {:#x}, R {:#x}", g.flags, r.flags),
        ));
    }
    if g.flags_1 != r.flags_1 {
        found.push(finding(
            "DT_FLAGS_1",
            None,
            format!("G {:#x}, R {:#x}", g.flags_1, r.flags_1),
        ));
    }
    if g.segments != r.segments {
        found.push(finding(
            "program headers",
            None,
            format!("G {}; R {}", g.segments.join(", "), r.segments.join(", ")),
        ));
    }
    for (kind, a, b) in [
        ("version definitions", &g.verdefs, &r.verdefs),
        ("version needs", &g.verneeds, &r.verneeds),
    ] {
        for v in a.difference(b) {
            found.push(finding(kind, None, format!("only in G: {v}")));
        }
        for v in b.difference(a) {
            found.push(finding(kind, None, format!("only in R: {v}")));
        }
    }
    if g.irelative != r.irelative {
        found.push(finding(
            "IRELATIVE relocations",
            None,
            format!("G {}, R {}", g.irelative, r.irelative),
        ));
    }
    symbols(g, r, &mut found);
    found
}

/// The differences between the dynamic symbols of two shared objects, G first.
fn symbols(g: &Library, r: &Library, found: &mut Vec<Finding>) {
    let used = Some("used, not defined");
    for (name, a) in &g.symbols {
        let Some(b) = r.symbols.get(name) else {
            let (kind, why) = if a.defined {
                ("symbol only in G", None)
            } else {
                ("symbol only in G", used)
            };
            found.push(finding(kind, why, format!("{name} {}", kind_name(a.kind))));
            continue;
        };
        let why = if a.defined || b.defined { None } else { used };
        if a.defined != b.defined {
            let side = |d: bool| if d { "defined" } else { "undefined" };
            found.push(finding(
                "symbol definition",
                None,
                format!("{name}: G {}, R {}", side(a.defined), side(b.defined)),
            ));
        }
        if a.binding != b.binding {
            found.push(finding(
                "symbol binding",
                why,
                format!(
                    "{name}: G {}, R {}",
                    binding_name(a.binding),
                    binding_name(b.binding)
                ),
            ));
        }
        if a.kind != b.kind {
            found.push(finding(
                "symbol type",
                why,
                format!("{name}: G {}, R {}", kind_name(a.kind), kind_name(b.kind)),
            ));
        }
        if a.visibility != b.visibility {
            found.push(finding(
                "symbol visibility",
                why,
                format!(
                    "{name}: G {}, R {}",
                    visibility_name(a.visibility),
                    visibility_name(b.visibility)
                ),
            ));
        }
        if a.size != b.size && a.defined && b.defined {
            let why = if is_function(a.kind) && is_function(b.kind) {
                Some("the size of a function")
            } else {
                None
            };
            found.push(finding(
                "symbol size",
                why,
                format!("{name} {}: G {}, R {}", kind_name(a.kind), a.size, b.size),
            ));
        }
    }
    for (name, b) in &r.symbols {
        if !g.symbols.contains_key(name) {
            let why = if b.defined { None } else { used };
            found.push(finding(
                "symbol only in R",
                why,
                format!("{name} {}", kind_name(b.kind)),
            ));
        }
    }
}

/// The `STT_GNU_IFUNC` symbols that each member of an archive defines, as `member symbol`.
pub fn archive_ifuncs(data: &[u8]) -> Result<BTreeSet<String>, String> {
    let archive = object::read::archive::ArchiveFile::parse(data).map_err(|e| e.to_string())?;
    let mut ifuncs = BTreeSet::new();
    for member in archive.members() {
        let member = member.map_err(|e| e.to_string())?;
        let name = String::from_utf8_lossy(member.name()).into_owned();
        let bytes = member.data(data).map_err(|e| format!("{name}: {e}"))?;
        let Ok(o) = objdiff::read(bytes) else {
            continue;
        };
        for s in o.symbols {
            if s.defined && s.kind == elf::STT_GNU_IFUNC.0 && s.binding != elf::STB_LOCAL.0 {
                ifuncs.insert(format!("{name} {}", s.name));
            }
        }
    }
    Ok(ifuncs)
}

/// The differences between the ifunc symbols of two archives, G first.
#[must_use]
pub fn archives(g: &BTreeSet<String>, r: &BTreeSet<String>) -> Vec<Finding> {
    let mut found = Vec::new();
    for s in g.difference(r) {
        found.push(finding("archive ifunc", None, format!("only in G: {s}")));
    }
    for s in r.difference(g) {
        found.push(finding("archive ifunc", None, format!("only in R: {s}")));
    }
    found
}

/// One file under a side's directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    /// A symbolic link, with what it points to.
    Link(String),
    /// A file.
    File(PathBuf),
}

/// Every file and link under `root`, by its path under `root`.
pub fn walk(root: &Path) -> Result<BTreeMap<String, Entry>, String> {
    let mut out = BTreeMap::new();
    let mut todo = vec![root.to_path_buf()];
    while let Some(dir) = todo.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            let key = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            if kind.is_symlink() {
                let target = std::fs::read_link(&path).map_err(|e| format!("{key}: {e}"))?;
                out.insert(key, Entry::Link(target.to_string_lossy().into_owned()));
            } else if kind.is_dir() {
                todo.push(path);
            } else if kind.is_file() {
                out.insert(key, Entry::File(path));
            }
        }
    }
    Ok(out)
}

/// The outcome of one pair of files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pair {
    /// The differences.
    Compared(Vec<Finding>),
    /// A file that could not be read, which counts.
    Unreadable(String),
}

/// Compare two files of the same path.
#[must_use]
#[allow(clippy::many_single_char_names)]
pub fn compare_files(g: &Path, r: &Path) -> Pair {
    let (a, b) = match (std::fs::read(g), std::fs::read(r)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) => return Pair::Unreadable(format!("{}: {e}", g.display())),
        (_, Err(e)) => return Pair::Unreadable(format!("{}: {e}", r.display())),
    };
    if a.starts_with(b"\x7fELF") || b.starts_with(b"\x7fELF") {
        return match (read(&a), read(&b)) {
            (Ok(x), Ok(y)) => Pair::Compared(libraries(&x, &y)),
            (Err(e), _) => Pair::Unreadable(format!("{}: {e}", g.display())),
            (_, Err(e)) => Pair::Unreadable(format!("{}: {e}", r.display())),
        };
    }
    if a.starts_with(b"!<arch>\n") || b.starts_with(b"!<arch>\n") {
        return match (archive_ifuncs(&a), archive_ifuncs(&b)) {
            (Ok(x), Ok(y)) => Pair::Compared(archives(&x, &y)),
            (Err(e), _) => Pair::Unreadable(format!("{}: {e}", g.display())),
            (_, Err(e)) => Pair::Unreadable(format!("{}: {e}", r.display())),
        };
    }
    if a == b {
        Pair::Compared(Vec::new())
    } else {
        Pair::Compared(vec![finding(
            "contents",
            None,
            "the file is not ELF and its bytes differ".into(),
        )])
    }
}

/// The comparison of two trees of libraries.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AbiDiff {
    /// Files both sides have.
    pub paired: usize,
    /// Files and links only G has.
    pub only_g: Vec<String>,
    /// Files and links only R has.
    pub only_r: Vec<String>,
    /// Files that could not be read.
    pub unreadable: Vec<String>,
    /// The differences by kind and reason, each with the path and the difference.
    pub found: BTreeMap<objdiff::Kind, Vec<(String, String)>>,
}

impl AbiDiff {
    /// The differences that count.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.only_g.len()
            + self.only_r.len()
            + self.unreadable.len()
            + self
                .found
                .iter()
                .filter(|((_, why), _)| why.is_none())
                .map(|(_, v)| v.len())
                .sum::<usize>()
    }

    /// The report. At most `limit` lines of each list are printed.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let mut s = String::new();
        let allowed: usize = self
            .found
            .iter()
            .filter(|((_, why), _)| why.is_some())
            .map(|(_, v)| v.len())
            .sum();
        let _ = writeln!(
            s,
            "rb abi-diff: files paired: {}, only in G: {}, only in R: {}. Counted differences: {}. Allowed differences: {allowed}.",
            self.paired,
            self.only_g.len(),
            self.only_r.len(),
            self.counted(),
        );
        let list = |s: &mut String, title: String, lines: &[(String, String)]| {
            let _ = writeln!(s, "  {title} ({}):", lines.len());
            for (path, what) in lines.iter().take(limit) {
                let _ = writeln!(s, "    {path}: {what}");
            }
            if lines.len() > limit {
                let _ = writeln!(s, "    and {} more", lines.len() - limit);
            }
        };
        if self.counted() > 0 {
            let _ = writeln!(s, "\nCounted differences:");
            for (title, paths) in [
                ("only in G", &self.only_g),
                ("only in R", &self.only_r),
                ("cannot read", &self.unreadable),
            ] {
                if !paths.is_empty() {
                    let lines: Vec<(String, String)> = paths
                        .iter()
                        .map(|p| (p.clone(), title.to_string()))
                        .collect();
                    list(&mut s, title.to_string(), &lines);
                }
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
        s
    }
}

/// Compare the trees `g` and `r`, `jobs` pairs at a time.
#[allow(clippy::many_single_char_names)]
pub fn compare(g: &Path, r: &Path, jobs: usize) -> Result<AbiDiff, String> {
    let a = walk(g)?;
    let b = walk(r)?;
    let mut diff = AbiDiff {
        only_g: a.keys().filter(|k| !b.contains_key(*k)).cloned().collect(),
        only_r: b.keys().filter(|k| !a.contains_key(*k)).cloned().collect(),
        ..AbiDiff::default()
    };
    let mut files = Vec::new();
    for (key, x) in &a {
        let Some(y) = b.get(key) else { continue };
        diff.paired += 1;
        match (x, y) {
            (Entry::Link(p), Entry::Link(q)) => {
                if p != q {
                    diff.found
                        .entry(("link", None))
                        .or_default()
                        .push((key.clone(), format!("G -> {p}, R -> {q}")));
                }
            }
            (Entry::File(p), Entry::File(q)) => files.push((key, p, q)),
            _ => diff.found.entry(("link", None)).or_default().push((
                key.clone(),
                "a link on one side and a file on the other".into(),
            )),
        }
    }
    let tasks: Vec<harness_agent::pool::Task<'_, Pair>> = files
        .iter()
        .map(|(_, p, q)| Box::new(move || compare_files(p, q)) as harness_agent::pool::Task<'_, _>)
        .collect();
    for ((key, _, _), pair) in files.iter().zip(harness_agent::pool::run(jobs, tasks)) {
        match pair {
            Pair::Unreadable(e) => diff.unreadable.push(e),
            Pair::Compared(found) => {
                for f in found {
                    diff.found
                        .entry((f.kind, f.allowed))
                        .or_default()
                        .push(((*key).clone(), f.what));
                }
            }
        }
    }
    Ok(diff)
}

/// `rb abi-diff` on the command line.
pub mod cli {
    use super::compare;
    use crate::cli::Args;
    use std::path::Path;

    /// Run the command. Returns the report and whether no difference counted.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err("give --g and --r, the directory of the libraries of each side".into());
        };
        let number = |key: &str, default: usize| -> Result<usize, String> {
            args.get(key).map_or(Ok(default), |n| {
                n.parse()
                    .map_err(|_| format!("--{key} {n} is not a number"))
            })
        };
        let diff = compare(
            Path::new(g),
            Path::new(r),
            number("jobs", harness_agent::pool::default_jobs(1))?,
        )?;
        let text = diff.report(number("limit", 20)?);
        Ok((text, diff.paired > 0 && diff.counted() == 0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::build::elf::{
        Builder, Dynamic, DynamicRelocation, SectionData, VersionData, VersionDef,
    };

    /// One dynamic symbol of a test library: its name, type, size, version, whether the version
    /// is hidden, and whether the library defines it.
    struct Sym {
        name: &'static str,
        kind: elf::SymbolType,
        size: u64,
        version: Option<&'static str>,
        hidden: bool,
        defined: bool,
    }

    fn sym(name: &'static str, kind: elf::SymbolType, size: u64) -> Sym {
        Sym {
            name,
            kind,
            size,
            version: Some("FBSD_1.0"),
            hidden: false,
            defined: true,
        }
    }

    /// The sections of a test library. It gives the id of `.text`.
    fn sections(b: &mut Builder<'_>, irelative: usize) -> object::build::elf::SectionId {
        let add = |b: &mut Builder<'_>, name: &'static [u8], kind, data| {
            let s = b.sections.add();
            s.name = name.into();
            s.sh_type = kind;
            s.sh_flags = elf::SHF_ALLOC;
            s.sh_addralign = 8;
            s.data = data;
            s.id()
        };
        let text = add(
            b,
            b".text",
            elf::SHT_PROGBITS,
            SectionData::Data(vec![0xc3; 64].into()),
        );
        let dynsym = add(b, b".dynsym", elf::SHT_DYNSYM, SectionData::DynamicSymbol);
        let dynstr = add(b, b".dynstr", elf::SHT_STRTAB, SectionData::DynamicString);
        let versym = add(
            b,
            b".gnu.version",
            elf::SHT_GNU_VERSYM,
            SectionData::GnuVersym,
        );
        let verdef = add(
            b,
            b".gnu.version_d",
            elf::SHT_GNU_VERDEF,
            SectionData::GnuVerdef,
        );
        let relocs = (0..irelative)
            .map(|i| DynamicRelocation {
                r_offset: 8 * i as u64,
                symbol: None,
                r_type: elf::R_X86_64_IRELATIVE,
                r_addend: 0,
            })
            .collect();
        let rela = add(
            b,
            b".rela.dyn",
            elf::SHT_RELA,
            SectionData::DynamicRelocation(relocs),
        );
        let dynamic = add(
            b,
            b".dynamic",
            elf::SHT_DYNAMIC,
            SectionData::Dynamic(vec![
                Dynamic::String {
                    tag: elf::DT_SONAME,
                    val: b"libt.so.1"[..].into(),
                },
                Dynamic::String {
                    tag: elf::DT_NEEDED,
                    val: b"libc.so.7"[..].into(),
                },
                Dynamic::Integer {
                    tag: elf::DT_FLAGS_1,
                    val: 1,
                },
                Dynamic::Integer {
                    tag: elf::DT_NULL,
                    val: 0,
                },
            ]),
        );
        for (id, link) in [
            (dynsym, dynstr),
            (versym, dynsym),
            (verdef, dynstr),
            (rela, dynsym),
            (dynamic, dynstr),
        ] {
            b.sections.get_mut(id).sh_link_section = Some(link);
        }
        let shstrtab = b.sections.add();
        shstrtab.name = b".shstrtab"[..].into();
        shstrtab.sh_type = elf::SHT_STRTAB;
        shstrtab.data = SectionData::SectionString;
        text
    }

    /// Lay out the alloc sections of a test library in one load segment, and write it.
    fn write(mut b: Builder<'_>) -> Vec<u8> {
        // The writer keeps the offsets of alloc sections, so lay them out in one load segment
        // after the file header and the program header.
        b.set_section_sizes();
        let alloc: Vec<_> = b
            .sections
            .iter()
            .filter(|s| s.is_alloc())
            .map(object::build::elf::Section::id)
            .collect();
        let seg = b.segments.add_load_segment(elf::PF_R, 0x1000).id();
        b.header.e_phoff = b.file_header_size();
        let head = b.file_header_size() + b.program_headers_size();
        let segment = b.segments.get_mut(seg);
        segment.p_filesz = head;
        segment.p_memsz = head;
        for id in alloc {
            b.segments
                .get_mut(seg)
                .append_section(b.sections.get_mut(id));
        }
        let mut out = Vec::new();
        b.write(&mut out).unwrap();
        out
    }

    /// An x86-64 shared object `libt.so.1` with the versions `FBSD_1.0` and `FBSD_1.1` (whose
    /// parent is `FBSD_1.0`), the symbols `syms`, and `irelative` IRELATIVE relocations.
    fn library(syms: &[Sym], irelative: usize) -> Vec<u8> {
        let mut b = Builder::new(Endianness::Little, true);
        b.header.e_type = elf::ET_DYN;
        b.header.e_machine = elf::EM_X86_64;
        b.version_base = Some(b"libt.so.1"[..].into());
        let v0 = b.versions.add(VersionData::Def(VersionDef {
            names: vec![b"FBSD_1.0"[..].into()],
            flags: elf::VersionFlags(0),
        }));
        let v1 = b.versions.add(VersionData::Def(VersionDef {
            names: vec![b"FBSD_1.1"[..].into(), b"FBSD_1.0"[..].into()],
            flags: elf::VersionFlags(0),
        }));
        let text = sections(&mut b, irelative);
        for s in syms {
            let d = b.dynamic_symbols.add();
            d.name = s.name.as_bytes().into();
            d.st_info = elf::SymbolInfo::new(elf::STB_GLOBAL, s.kind);
            d.st_size = s.size;
            if s.defined {
                d.section = Some(text);
            }
            d.version = match s.version {
                Some("FBSD_1.1") => v1,
                Some(_) => v0,
                None => object::build::elf::VersionId::global(),
            };
            d.version_hidden = s.hidden;
        }
        write(b)
    }

    fn kinds(found: &[Finding]) -> Vec<(&'static str, bool)> {
        found
            .iter()
            .map(|f| (f.kind, f.allowed.is_none()))
            .collect()
    }

    #[test]
    fn a_library_reads_back() {
        let mut compat = sym("f", elf::STT_FUNC, 8);
        compat.hidden = true;
        let mut new = sym("f", elf::STT_FUNC, 16);
        new.version = Some("FBSD_1.1");
        let lib = read(&library(
            &[
                compat,
                new,
                sym("table", elf::STT_OBJECT, 32),
                sym("memcpy", elf::STT_GNU_IFUNC, 4),
            ],
            2,
        ))
        .unwrap();
        assert_eq!(lib.soname, "libt.so.1");
        assert_eq!(lib.needed, ["libc.so.7"]);
        assert_eq!(lib.flags_1, 1);
        assert_eq!(lib.irelative, 2);
        assert_eq!(
            lib.verdefs.iter().map(String::as_str).collect::<Vec<_>>(),
            ["FBSD_1.0", "FBSD_1.1: FBSD_1.0", "base libt.so.1"]
        );
        assert_eq!(
            lib.symbols.keys().map(String::as_str).collect::<Vec<_>>(),
            [
                "f@@FBSD_1.1",
                "f@FBSD_1.0",
                "memcpy@@FBSD_1.0",
                "table@@FBSD_1.0"
            ]
        );
        assert_eq!(lib.symbols["memcpy@@FBSD_1.0"].kind, elf::STT_GNU_IFUNC.0);
        assert_eq!(lib.symbols["table@@FBSD_1.0"].size, 32);
    }

    #[test]
    fn a_compat_symbol_without_its_hidden_version_counts() {
        let mut compat = sym("f", elf::STT_FUNC, 8);
        compat.hidden = true;
        let g = read(&library(&[compat, sym("g", elf::STT_FUNC, 8)], 0)).unwrap();
        // R exports f at the default version, as a lost .symver would.
        let r = read(&library(
            &[sym("f", elf::STT_FUNC, 8), sym("g", elf::STT_FUNC, 8)],
            0,
        ))
        .unwrap();
        let found = libraries(&g, &r);
        assert_eq!(
            kinds(&found),
            [("symbol only in G", true), ("symbol only in R", true)]
        );
        assert_eq!(found[0].what, "f@FBSD_1.0 FUNC");
        assert_eq!(found[1].what, "f@@FBSD_1.0 FUNC");
    }

    #[test]
    fn data_sizes_count_and_function_sizes_do_not() {
        let g = read(&library(
            &[
                sym("f", elf::STT_FUNC, 8),
                sym("table", elf::STT_OBJECT, 32),
                sym("memcpy", elf::STT_GNU_IFUNC, 4),
            ],
            1,
        ))
        .unwrap();
        let r = read(&library(
            &[
                sym("f", elf::STT_FUNC, 12),
                sym("table", elf::STT_OBJECT, 24),
                sym("memcpy", elf::STT_FUNC, 4),
            ],
            0,
        ))
        .unwrap();
        let mut found = kinds(&libraries(&g, &r));
        found.sort_unstable();
        assert_eq!(
            found,
            [
                ("IRELATIVE relocations", true),
                ("symbol size", false),
                ("symbol size", true),
                ("symbol type", true),
            ]
        );
    }

    #[test]
    fn a_used_symbol_is_allowed_to_differ() {
        let mut used = sym("strlen", elf::STT_FUNC, 0);
        used.defined = false;
        used.version = None;
        let g = read(&library(&[sym("f", elf::STT_FUNC, 8)], 0)).unwrap();
        let r = read(&library(&[sym("f", elf::STT_FUNC, 8), used], 0)).unwrap();
        assert_eq!(kinds(&libraries(&g, &r)), [("symbol only in R", false)]);
        assert!(libraries(&g, &g).is_empty());
    }

    #[test]
    fn trees_pair_by_path_and_count_a_missing_file() {
        let dir = std::env::temp_dir().join(format!("rb-abi-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for side in ["g", "r"] {
            let lib = dir.join(side).join("lib");
            std::fs::create_dir_all(&lib).unwrap();
            let size = if side == "g" { 32 } else { 24 };
            std::fs::write(
                lib.join("libt.so.1"),
                library(&[sym("table", elf::STT_OBJECT, size)], 0),
            )
            .unwrap();
            std::fs::write(lib.join("libt.so"), "GROUP ( /lib/libt.so.1 )\n").unwrap();
        }
        std::fs::write(dir.join("g/lib/libonly.so.1"), b"x").unwrap();
        let diff = compare(&dir.join("g"), &dir.join("r"), 2).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(diff.paired, 2);
        assert_eq!(diff.only_g, ["lib/libonly.so.1"]);
        assert_eq!(diff.counted(), 2);
        let report = diff.report(20);
        assert!(
            report.contains("lib/libt.so.1: table@@FBSD_1.0 OBJECT: G 32, R 24"),
            "{report}"
        );
    }
}
