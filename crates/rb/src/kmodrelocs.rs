//! `rb kmod-relocs`: the relocations of each kernel module are of the types the kernel linker
//! takes (plan 09.3).
//!
//! A FreeBSD module on amd64 is an `ld -r` object (`ET_REL`), and `link_elf_obj.c` loads it. It
//! applies only the relocation sections whose target section is `SHF_ALLOC`, and
//! `elf_reloc_internal` of `sys/amd64/amd64/elf_machdep.c` applies only these types: `NONE`,
//! `64`, `PC32`, `PLT32`, `32S`, `GLOB_DAT`, `JUMP_SLOT`, `RELATIVE` and `IRELATIVE`. For any
//! other type `kldload` prints "unexpected relocation type" and refuses the module. So a
//! compiler that emits, for example, `R_X86_64_32` or `GOTPCRELX` in a module makes a module
//! that builds and does not load.
//!
//! Every `*.ko` file under the directory is read. A relocation of another type in a section the
//! kernel linker applies is a finding. A file that is not an x86-64 `ET_REL` object is listed and
//! not counted.

use object::Endianness;
use object::elf;
use object::read::elf::{FileHeader, SectionHeader as _, Sym as _};
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// The relocation types `elf_reloc_internal` of FreeBSD amd64 applies.
pub const ACCEPTED: [elf::RelocationType; 9] = [
    elf::R_X86_64_NONE,
    elf::R_X86_64_64,
    elf::R_X86_64_PC32,
    elf::R_X86_64_PLT32,
    elf::R_X86_64_32S,
    elf::R_X86_64_GLOB_DAT,
    elf::R_X86_64_JUMP_SLOT,
    elf::R_X86_64_RELATIVE,
    elf::R_X86_64_IRELATIVE,
];

/// The name of an x86-64 relocation type, as readelf gives it without `R_X86_64_`.
#[must_use]
pub fn type_name(t: elf::RelocationType) -> String {
    let name = match t {
        elf::R_X86_64_NONE => "NONE",
        elf::R_X86_64_64 => "64",
        elf::R_X86_64_PC32 => "PC32",
        elf::R_X86_64_GOT32 => "GOT32",
        elf::R_X86_64_PLT32 => "PLT32",
        elf::R_X86_64_COPY => "COPY",
        elf::R_X86_64_GLOB_DAT => "GLOB_DAT",
        elf::R_X86_64_JUMP_SLOT => "JUMP_SLOT",
        elf::R_X86_64_RELATIVE => "RELATIVE",
        elf::R_X86_64_GOTPCREL => "GOTPCREL",
        elf::R_X86_64_32 => "32",
        elf::R_X86_64_32S => "32S",
        elf::R_X86_64_16 => "16",
        elf::R_X86_64_PC16 => "PC16",
        elf::R_X86_64_8 => "8",
        elf::R_X86_64_PC8 => "PC8",
        elf::R_X86_64_DTPMOD64 => "DTPMOD64",
        elf::R_X86_64_DTPOFF64 => "DTPOFF64",
        elf::R_X86_64_TPOFF64 => "TPOFF64",
        elf::R_X86_64_TLSGD => "TLSGD",
        elf::R_X86_64_TLSLD => "TLSLD",
        elf::R_X86_64_DTPOFF32 => "DTPOFF32",
        elf::R_X86_64_GOTTPOFF => "GOTTPOFF",
        elf::R_X86_64_TPOFF32 => "TPOFF32",
        elf::R_X86_64_PC64 => "PC64",
        elf::R_X86_64_GOTOFF64 => "GOTOFF64",
        elf::R_X86_64_GOTPC32 => "GOTPC32",
        elf::R_X86_64_SIZE32 => "SIZE32",
        elf::R_X86_64_SIZE64 => "SIZE64",
        elf::R_X86_64_IRELATIVE => "IRELATIVE",
        elf::R_X86_64_GOTPCRELX => "GOTPCRELX",
        elf::R_X86_64_REX_GOTPCRELX => "REX_GOTPCRELX",
        _ => return format!("type {}", t.0),
    };
    name.to_string()
}

/// The relocations of one module.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Module {
    /// The count of each type in the sections the kernel linker applies.
    pub types: BTreeMap<elf::RelocationType, usize>,
    /// The relocations of a type the kernel linker refuses: the section, the type and the symbol.
    pub refused: Vec<(String, elf::RelocationType, String)>,
}

/// Read the relocations of one module. `Ok(None)` when it is not an x86-64 `ET_REL` object.
pub fn read(data: &[u8]) -> Result<Option<Module>, String> {
    let bad = |e: object::Error| e.to_string();
    let Ok(header) = elf::FileHeader64::<Endianness>::parse(data) else {
        return Ok(None);
    };
    let endian = header.endian().map_err(bad)?;
    if header.e_machine(endian) != elf::EM_X86_64 || header.e_type(endian) != elf::ET_REL {
        return Ok(None);
    }
    let sections = header.sections(endian, data).map_err(bad)?;
    let symbols = sections
        .symbols(endian, data, elf::SHT_SYMTAB)
        .map_err(bad)?;
    let symbol = |index: u32| {
        symbols
            .symbol(object::SymbolIndex(index as usize))
            .ok()
            .and_then(|s| s.name(endian, symbols.strings()).ok())
            .map(|n| String::from_utf8_lossy(n).into_owned())
            .unwrap_or_default()
    };
    let mut module = Module::default();
    for header in sections.iter() {
        let kind = header.sh_type(endian);
        if kind != elf::SHT_RELA && kind != elf::SHT_REL {
            continue;
        }
        let target = sections
            .section(object::SectionIndex(header.sh_info(endian) as usize))
            .map_err(bad)?;
        if target.sh_flags(endian).0 & elf::SHF_ALLOC.0 == 0 {
            continue;
        }
        let name = String::from_utf8_lossy(sections.section_name(endian, header).map_err(bad)?)
            .into_owned();
        let mut list: Vec<(elf::RelocationType, u32)> = Vec::new();
        if kind == elf::SHT_RELA {
            if let Some((relas, _)) = header.rela(endian, data).map_err(bad)? {
                list.extend(
                    relas
                        .iter()
                        .map(|r| (r.r_type(endian, false), r.r_sym(endian, false))),
                );
            }
        } else if let Some((rels, _)) = header.rel(endian, data).map_err(bad)? {
            list.extend(rels.iter().map(|r| (r.r_type(endian), r.r_sym(endian))));
        }
        for (t, sym) in list {
            *module.types.entry(t).or_default() += 1;
            if !ACCEPTED.contains(&t) {
                module.refused.push((name.clone(), t, symbol(sym)));
            }
        }
    }
    Ok(Some(module))
}

/// What `rb kmod-relocs` found under one directory.
#[derive(Debug, Default)]
pub struct Found {
    /// The modules read, by their path under the directory.
    pub modules: BTreeMap<String, Module>,
    /// The files that are not x86-64 `ET_REL` objects.
    pub other: Vec<String>,
    /// The files that could not be read, with the reason.
    pub unreadable: Vec<String>,
}

impl Found {
    /// The modules with a relocation the kernel linker refuses.
    #[must_use]
    pub fn refused(&self) -> usize {
        self.modules
            .values()
            .filter(|m| !m.refused.is_empty())
            .count()
    }

    /// Whether modules were read and none has a refused relocation or could not be read.
    #[must_use]
    pub fn clean(&self) -> bool {
        !self.modules.is_empty() && self.refused() == 0 && self.unreadable.is_empty()
    }

    /// The report, with each list stopped after `limit` lines.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let mut totals: BTreeMap<elf::RelocationType, usize> = BTreeMap::new();
        for m in self.modules.values() {
            for (t, n) in &m.types {
                *totals.entry(*t).or_default() += n;
            }
        }
        let mut s = String::new();
        let _ = writeln!(
            s,
            "rb kmod-relocs: modules read: {}, not x86-64 ET_REL: {}, unreadable: {}. Findings: {} modules with a relocation the kernel linker refuses.",
            self.modules.len(),
            self.other.len(),
            self.unreadable.len(),
            self.refused(),
        );
        let types: Vec<String> = totals
            .iter()
            .map(|(t, n)| format!("{}={n}", type_name(*t)))
            .collect();
        let _ = writeln!(
            s,
            "Relocations in the sections the kernel linker applies: {}",
            if types.is_empty() {
                "none".to_string()
            } else {
                types.join(" ")
            }
        );
        let list = |s: &mut String, title: &str, lines: &[String]| {
            if lines.is_empty() {
                return;
            }
            let _ = writeln!(s, "\n{title} ({}):", lines.len());
            for l in lines.iter().take(limit) {
                let _ = writeln!(s, "  {l}");
            }
            if lines.len() > limit {
                let _ = writeln!(s, "  and {} more", lines.len() - limit);
            }
        };
        let refused: Vec<String> = self
            .modules
            .iter()
            .flat_map(|(path, m)| {
                m.refused
                    .iter()
                    .map(move |(sec, t, sym)| format!("{path}: {sec}: {} {sym}", type_name(*t)))
            })
            .collect();
        list(&mut s, "Relocations the kernel linker refuses", &refused);
        list(&mut s, "Unreadable", &self.unreadable);
        list(
            &mut s,
            "Not x86-64 ET_REL objects, not counted",
            &self.other,
        );
        s
    }
}

/// Every `*.ko` file under `dir`, in path order.
fn modules(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let mut todo = vec![dir.to_path_buf()];
    while let Some(d) = todo.pop() {
        let entries = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
        for entry in entries {
            let entry = entry.map_err(|e| format!("{}: {e}", d.display()))?;
            let path = entry.path();
            let kind = entry
                .file_type()
                .map_err(|e| format!("{}: {e}", path.display()))?;
            if kind.is_dir() {
                todo.push(path);
            } else if kind.is_file() && path.extension().is_some_and(|e| e == "ko") {
                out.push(path);
            }
        }
    }
    out.sort();
    Ok(out)
}

/// Read every module under `dir`.
pub fn run(dir: &Path) -> Result<Found, String> {
    let mut found = Found::default();
    for path in modules(dir)? {
        let key = path
            .strip_prefix(dir)
            .unwrap_or(&path)
            .display()
            .to_string();
        match std::fs::read(&path)
            .map_err(|e| e.to_string())
            .and_then(|data| read(&data))
        {
            Ok(Some(m)) => {
                found.modules.insert(key, m);
            }
            Ok(None) => found.other.push(key),
            Err(e) => found.unreadable.push(format!("{key}: {e}")),
        }
    }
    Ok(found)
}

/// The command line of `rb kmod-relocs`.
pub mod cli {
    use crate::cli::Args;
    use std::path::Path;

    /// Run `rb kmod-relocs`. Returns the report and whether it is clean.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let dir = args
            .target
            .as_deref()
            .ok_or("name the directory of the modules")?;
        let limit = args.get("limit").map_or(Ok(40), |n| {
            n.parse()
                .map_err(|_| format!("--limit {n} is not a number"))
        })?;
        let found = super::run(Path::new(dir))?;
        Ok((found.report(limit), found.clean()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object as Writer, Relocation, Symbol as WSymbol, SymbolSection};
    use object::{
        Architecture, BinaryFormat, RelocationFlags, SectionKind, SymbolFlags, SymbolKind,
        SymbolScope,
    };

    /// An x86-64 relocatable object with a relocation of each type of `text` in `.text`, and one
    /// of each type of `debug` in `.debug_info`, all against the symbol `ext`.
    fn object(text: &[elf::RelocationType], debug: &[elf::RelocationType]) -> Vec<u8> {
        let mut obj = Writer::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let ext = obj.add_symbol(WSymbol {
            name: b"ext".to_vec(),
            value: 0,
            size: 0,
            kind: SymbolKind::Data,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Undefined,
            flags: SymbolFlags::None,
        });
        let code = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        obj.append_section_data(code, &[0x90; 64], 16);
        let info = obj.add_section(Vec::new(), b".debug_info".to_vec(), SectionKind::Debug);
        obj.append_section_data(info, &[0; 64], 1);
        for (section, types) in [(code, text), (info, debug)] {
            for (i, t) in types.iter().enumerate() {
                obj.add_relocation(
                    section,
                    Relocation {
                        offset: 8 * i as u64,
                        symbol: ext,
                        addend: 0,
                        flags: RelocationFlags::Elf { r_type: *t },
                    },
                )
                .unwrap();
            }
        }
        obj.write().unwrap()
    }

    #[test]
    fn the_types_the_kernel_linker_applies_are_counted() {
        let m = read(&object(
            &[elf::R_X86_64_PLT32, elf::R_X86_64_PLT32, elf::R_X86_64_32S],
            &[],
        ))
        .unwrap()
        .unwrap();
        assert_eq!(m.types[&elf::R_X86_64_PLT32], 2);
        assert_eq!(m.types[&elf::R_X86_64_32S], 1);
        assert!(m.refused.is_empty());
    }

    #[test]
    fn another_type_in_an_allocated_section_is_refused() {
        let m = read(&object(
            &[
                elf::R_X86_64_64,
                elf::R_X86_64_32,
                elf::R_X86_64_REX_GOTPCRELX,
            ],
            &[],
        ))
        .unwrap()
        .unwrap();
        assert_eq!(
            m.refused,
            vec![
                (
                    ".rela.text".to_string(),
                    elf::R_X86_64_32,
                    "ext".to_string()
                ),
                (
                    ".rela.text".to_string(),
                    elf::R_X86_64_REX_GOTPCRELX,
                    "ext".to_string()
                ),
            ]
        );
    }

    #[test]
    fn a_section_the_kernel_linker_does_not_load_is_left_out() {
        let m = read(&object(&[], &[elf::R_X86_64_32, elf::R_X86_64_32]))
            .unwrap()
            .unwrap();
        assert!(m.types.is_empty());
        assert!(m.refused.is_empty());
    }

    #[test]
    fn a_tree_of_modules_is_read_and_reported() {
        let dir = std::env::temp_dir().join(format!("rb-kmod-relocs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("a")).unwrap();
        std::fs::write(dir.join("a/good.ko"), object(&[elf::R_X86_64_PC32], &[])).unwrap();
        std::fs::write(dir.join("bad.ko"), object(&[elf::R_X86_64_32], &[])).unwrap();
        std::fs::write(dir.join("text.ko"), b"not an object").unwrap();
        std::fs::write(dir.join("good.o"), object(&[elf::R_X86_64_32], &[])).unwrap();
        let found = run(&dir).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(found.modules.len(), 2);
        assert_eq!(found.other, vec!["text.ko".to_string()]);
        assert_eq!(found.refused(), 1);
        assert!(!found.clean());
        let report = found.report(10);
        assert!(report.starts_with(
            "rb kmod-relocs: modules read: 2, not x86-64 ET_REL: 1, unreadable: 0. Findings: 1"
        ));
        assert!(report.contains("PC32=1 32=1"), "{report}");
        assert!(report.contains("  bad.ko: .rela.text: 32 ext"), "{report}");
    }

    #[test]
    fn a_tree_with_no_module_is_not_clean() {
        assert!(!Found::default().clean());
        let mut found = Found::default();
        found.modules.insert("a.ko".into(), Module::default());
        assert!(found.clean());
    }
}
