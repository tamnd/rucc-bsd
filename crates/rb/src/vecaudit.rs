//! `rb vec-audit`: no vector or x87 instruction in an object built without them (plan 08.3).
//!
//! Both kernels on amd64 are built with no SSE and no x87 (`-mno-mmx -mno-sse -mno-avx
//! -msoft-float`), and neither saves the user's FPU registers on kernel entry unless the code asks
//! for it with `fpu_kern_enter`. A compiler that takes the flags and still emits SSE, for a struct
//! copy or a constant, corrupts a user process's registers with no crash to show for it.
//!
//! Each object of R whose compile command leaves the unit without the FPU is decoded. Each
//! instruction that names an `xmm`, `ymm`, `zmm`, `mm`, `st`, mask or tile register, or that is an
//! x87 instruction, is counted by mnemonic and by the function it is in. The files that use the
//! FPU by design do it in inline asm or in assembler, which G's object of the same unit has too.
//! So a mnemonic in R's object is allowed when G's object of the same unit has it, and is a
//! finding when it does not. An object only R made is judged alone. Objects are paired by path as
//! for `rb sections-diff`. An object for another machine than x86-64 is skipped.

use crate::objdiff::{Unit, unit};
use harness_shim::record::CompileRecord;
use iced_x86::{Decoder, DecoderOptions, Instruction, Register};
use object::{Object as _, ObjectSection as _, ObjectSymbol as _};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// The flagged instructions of one object: mnemonic, then function, then count.
pub type Audit = BTreeMap<String, BTreeMap<String, u64>>;

/// Whether a compile command leaves the unit without the FPU and the vector registers. The last
/// word that decides wins, as the compiler reads them.
#[must_use]
pub fn no_fpu(words: &[String]) -> bool {
    let mut off = false;
    for w in words {
        match w.as_str() {
            "-mno-sse" | "-mno-80387" | "-mgeneral-regs-only" | "-msoft-float" => off = true,
            "-msse" | "-msse2" | "-mavx" | "-mavx2" | "-mhard-float" | "-m80387" => off = false,
            _ => {}
        }
    }
    off
}

/// The objects of a build that leave out the FPU, keyed as in [`crate::objdiff::units`]. When
/// two calls made the same object, the last one counts.
#[must_use]
pub fn no_fpu_units(records: &[CompileRecord], root: &str) -> BTreeMap<String, Unit> {
    let mut out = BTreeMap::new();
    for r in records {
        if let Some((key, u)) = unit(r, root) {
            if no_fpu(&r.argv) {
                out.insert(key, u);
            } else {
                out.remove(&key);
            }
        }
    }
    out
}

fn is_vector(r: Register) -> bool {
    r.is_xmm() || r.is_ymm() || r.is_zmm() || r.is_mm() || r.is_st() || r.is_k() || r.is_tmm()
}

/// Whether an instruction uses the FPU or a vector register.
#[must_use]
pub fn flagged(instr: &Instruction) -> bool {
    (0..instr.op_count()).any(|i| is_vector(instr.op_register(i)))
        || is_vector(instr.memory_index())
        || instr
            .cpuid_features()
            .iter()
            .any(|f| format!("{f:?}").contains("FPU"))
}

/// Count the flagged instructions of x86-64 code. `functions` holds the start, size and name of
/// each function in the code, sorted by start. An instruction in no function is put under
/// `section`.
pub fn count_code(code: &[u8], functions: &[(u64, u64, String)], section: &str, out: &mut Audit) {
    let mut decoder = Decoder::new(64, code, DecoderOptions::NONE);
    let mut instr = Instruction::default();
    while decoder.can_decode() {
        let at = decoder.position() as u64;
        decoder.decode_out(&mut instr);
        if instr.is_invalid() || !flagged(&instr) {
            continue;
        }
        let function = functions
            .iter()
            .rev()
            .find(|(start, size, _)| *start <= at && at < start + (*size).max(1))
            .map_or(section, |(_, _, name)| name.as_str());
        *out.entry(format!("{:?}", instr.mnemonic()).to_lowercase())
            .or_default()
            .entry(function.to_string())
            .or_default() += 1;
    }
}

/// The flagged instructions of one object, or `None` when it is not for x86-64.
pub fn read(data: &[u8]) -> Result<Option<Audit>, String> {
    let file = object::File::parse(data).map_err(|e| e.to_string())?;
    if file.architecture() != object::Architecture::X86_64 {
        return Ok(None);
    }
    let mut out = Audit::new();
    for section in file.sections() {
        if section.kind() != object::SectionKind::Text {
            continue;
        }
        let code = section.data().map_err(|e| e.to_string())?;
        let mut functions: Vec<(u64, u64, String)> = file
            .symbols()
            .filter(|s| {
                s.section_index() == Some(section.index()) && s.kind() == object::SymbolKind::Text
            })
            .filter_map(|s| Some((s.address(), s.size(), s.name().ok()?.to_string())))
            .collect();
        functions.sort();
        count_code(code, &functions, section.name().unwrap_or("?"), &mut out);
    }
    Ok(Some(out))
}

/// What one object of R gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pair {
    /// R's audit, and G's when G made the object too.
    Read(Audit, Option<Audit>),
    /// R's object is not for x86-64.
    Skipped,
    /// R's object is gone, as when the build removed a temporary object.
    Gone,
    /// An object cannot be read, with why.
    Unreadable(String),
}

/// Read R's object and, when there is one, G's. NetBSD compiles each unit of libkern to `X.o.o`,
/// links it to `X.o` with `ld -x -r` and removes `X.o.o`, so `X.o` is read when `X.o.o` is gone.
#[must_use]
pub fn audit_pair(g: Option<&str>, r: &str) -> Pair {
    let load = |path: &str| {
        let mut file = std::fs::read(path);
        if let (Err(e), Some(linked)) = (&file, path.strip_suffix(".o.o"))
            && e.kind() == std::io::ErrorKind::NotFound
        {
            file = std::fs::read(format!("{linked}.o"));
        }
        match file {
            Ok(data) => read(&data).map(Some).map_err(|e| format!("{path}: {e}")),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{path}: {e}")),
        }
    };
    let r = match load(r) {
        Err(e) => return Pair::Unreadable(e),
        Ok(None) => return Pair::Gone,
        Ok(Some(None)) => return Pair::Skipped,
        Ok(Some(Some(a))) => a,
    };
    match g.map(load) {
        Some(Err(e)) => Pair::Unreadable(e),
        Some(Ok(Some(Some(a)))) => Pair::Read(r, Some(a)),
        _ => Pair::Read(r, None),
    }
}

/// The audit of R's build.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VecAudit {
    /// R's objects that were decoded.
    pub decoded: usize,
    /// Of those, the ones G made too.
    pub paired: usize,
    /// R's objects for another machine.
    pub skipped: usize,
    /// R's objects no longer on disk.
    pub gone: usize,
    /// Objects that could not be read, which count.
    pub unreadable: Vec<String>,
    /// The findings: the unit's source, the mnemonic, and the functions with their counts.
    pub findings: Vec<(String, String, BTreeMap<String, u64>)>,
    /// The allowed ones: the unit's source, the mnemonic, and the count in R and in G.
    pub allowed: Vec<(String, String, u64, u64)>,
}

impl VecAudit {
    /// What counts: the findings and the objects that could not be read.
    #[must_use]
    pub fn counted(&self) -> usize {
        self.findings.len() + self.unreadable.len()
    }

    /// Put the audit of one object of R in.
    pub fn add(&mut self, source: &str, r: &Audit, g: Option<&Audit>) {
        self.decoded += 1;
        if g.is_some() {
            self.paired += 1;
        }
        for (mnemonic, functions) in r {
            let total = functions.values().sum();
            match g.and_then(|g| g.get(mnemonic)) {
                Some(theirs) => self.allowed.push((
                    source.to_string(),
                    mnemonic.clone(),
                    total,
                    theirs.values().sum(),
                )),
                None => {
                    self.findings
                        .push((source.to_string(), mnemonic.clone(), functions.clone()));
                }
            }
        }
    }

    /// The report. Each list stops after `limit` lines.
    #[must_use]
    pub fn report(&self, limit: usize) -> String {
        let mut s = String::new();
        let _ = writeln!(
            s,
            "rb vec-audit: no-FPU objects of R decoded: {}, paired with G: {}, not x86-64: {}, no longer on disk: {}. Findings: {}. Allowed, as G's object has them too: {}.",
            self.decoded,
            self.paired,
            self.skipped,
            self.gone,
            self.counted(),
            self.allowed.len(),
        );
        let more = |s: &mut String, n: usize| {
            if n > limit {
                let _ = writeln!(s, "  and {} more", n - limit);
            }
        };
        if !self.unreadable.is_empty() {
            let _ = writeln!(s, "\nObjects that cannot be read:");
            for e in self.unreadable.iter().take(limit) {
                let _ = writeln!(s, "  {e}");
            }
            more(&mut s, self.unreadable.len());
        }
        if !self.findings.is_empty() {
            let _ = writeln!(
                s,
                "\nVector or x87 instructions that G's object does not have:"
            );
            for (source, mnemonic, functions) in self.findings.iter().take(limit) {
                let list: Vec<String> =
                    functions.iter().map(|(f, n)| format!("{f} x{n}")).collect();
                let _ = writeln!(s, "  {source}: {mnemonic} in {}", list.join(", "));
            }
            more(&mut s, self.findings.len());
        }
        if !self.allowed.is_empty() {
            let _ = writeln!(s, "\nAllowed, not counted, with the count in R and in G:");
            for (source, mnemonic, r, g) in self.allowed.iter().take(limit) {
                let _ = writeln!(s, "  {source}: {mnemonic} R x{r}, G x{g}");
            }
            more(&mut s, self.allowed.len());
        }
        s
    }
}

/// Audit the no-FPU objects of R in `r` against the objects of G in `g`, `jobs` at a time.
#[must_use]
pub fn audit(g: &BTreeMap<String, Unit>, r: &BTreeMap<String, Unit>, jobs: usize) -> VecAudit {
    let pairs: Vec<(&String, &Unit, Option<&Unit>)> =
        r.iter().map(|(k, u)| (k, u, g.get(k))).collect();
    let tasks: Vec<harness_agent::pool::Task<'_, Pair>> = pairs
        .iter()
        .map(|(_, r, g)| {
            Box::new(move || audit_pair(g.map(|u| u.path.as_str()), &r.path))
                as harness_agent::pool::Task<'_, _>
        })
        .collect();
    let mut out = VecAudit::default();
    for ((key, u, _), pair) in pairs.iter().zip(harness_agent::pool::run(jobs, tasks)) {
        let source = if u.source.is_empty() {
            key.as_str()
        } else {
            u.source.as_str()
        };
        match pair {
            Pair::Read(r, g) => out.add(source, &r, g.as_ref()),
            Pair::Skipped => out.skipped += 1,
            Pair::Gone => out.gone += 1,
            Pair::Unreadable(e) => out.unreadable.push(e),
        }
    }
    out
}

/// `rb vec-audit` on the command line.
pub mod cli {
    use super::{audit, no_fpu_units};
    use crate::cli::Args;
    use crate::objdiff::units;
    use harness_shim::record::read_log;
    use std::path::Path;

    /// Run `rb vec-audit`. Returns the report and whether objects were decoded and nothing
    /// counted.
    pub fn run(args: &Args) -> Result<(String, bool), String> {
        let (Some(g), Some(r)) = (args.get("g"), args.get("r")) else {
            return Err("give --g and --r, the compile.jsonl of each build".into());
        };
        let number = |key: &str, default: usize| -> Result<usize, String> {
            args.get(key).map_or(Ok(default), |n| {
                n.parse()
                    .map_err(|_| format!("--{key} {n} is not a number"))
            })
        };
        let read = |log: &str| {
            let (records, _) = read_log(Path::new(log)).map_err(|e| format!("{log}: {e}"))?;
            Ok::<_, String>(records)
        };
        let g_units = units(&read(g)?, args.get("g-root").unwrap_or_default());
        let r_units = no_fpu_units(&read(r)?, args.get("r-root").unwrap_or_default());
        let found = audit(
            &g_units,
            &r_units,
            number("jobs", harness_agent::pool::default_jobs(1))?,
        );
        Ok((
            found.report(number("limit", 20)?),
            found.decoded > 0 && found.counted() == 0,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object as Writer, Symbol as WSymbol, SymbolSection};
    use object::{
        Architecture, BinaryFormat, Endianness, SectionKind, SymbolFlags, SymbolKind, SymbolScope,
    };

    /// An x86-64 object with one function of each name and code in `.text`.
    fn object(functions: &[(&str, &[u8])]) -> Vec<u8> {
        let mut obj = Writer::new(BinaryFormat::Elf, Architecture::X86_64, Endianness::Little);
        let text = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        for (name, code) in functions {
            let at = obj.append_section_data(text, code, 1);
            obj.add_symbol(WSymbol {
                name: name.as_bytes().to_vec(),
                value: at,
                size: code.len() as u64,
                kind: SymbolKind::Text,
                scope: SymbolScope::Linkage,
                weak: false,
                section: SymbolSection::Section(text),
                flags: SymbolFlags::None,
            });
        }
        obj.write().unwrap()
    }

    fn words(s: &str) -> Vec<String> {
        s.split_whitespace().map(str::to_string).collect()
    }

    #[test]
    fn the_last_flag_decides() {
        assert!(no_fpu(&words(
            "gcc -mno-mmx -mno-sse -mno-avx -msoft-float -c a.c"
        )));
        assert!(!no_fpu(&words("gcc -mno-sse -msse2 -c a.c")));
        assert!(!no_fpu(&words("gcc -O2 -c a.c")));
    }

    // mov rbx, rax; movaps xmm0, xmm1; ret
    const SSE: &[u8] = &[0x48, 0x89, 0xc3, 0x0f, 0x28, 0xc1, 0xc3];
    // fldz; fld qword [rax]; lfence; ret
    const X87: &[u8] = &[0xd9, 0xee, 0xdd, 0x00, 0x0f, 0xae, 0xe8, 0xc3];
    // mov rbx, rax; ret
    const PLAIN: &[u8] = &[0x48, 0x89, 0xc3, 0xc3];

    #[test]
    fn sse_and_x87_are_counted_by_function_and_plain_code_is_not() {
        let audit = read(&object(&[("copy", SSE), ("plain", PLAIN), ("fpu", X87)]))
            .unwrap()
            .unwrap();
        let got: Vec<(&str, Vec<(&str, u64)>)> = audit
            .iter()
            .map(|(m, f)| {
                (
                    m.as_str(),
                    f.iter().map(|(n, c)| (n.as_str(), *c)).collect(),
                )
            })
            .collect();
        assert_eq!(
            got,
            [
                ("fld", vec![("fpu", 1)]),
                ("fldz", vec![("fpu", 1)]),
                ("movaps", vec![("copy", 1)])
            ]
        );
    }

    #[test]
    fn a_mnemonic_g_has_too_is_allowed() {
        let r = read(&object(&[("copy", SSE), ("fpu", X87)]))
            .unwrap()
            .unwrap();
        let g = read(&object(&[("copy", PLAIN), ("fpu", X87)]))
            .unwrap()
            .unwrap();
        let mut found = VecAudit::default();
        found.add("/src/a.c", &r, Some(&g));
        assert_eq!(found.counted(), 1);
        assert_eq!(found.allowed.len(), 2);
        let text = found.report(20);
        assert!(text.contains("  /src/a.c: movaps in copy x1\n"), "{text}");
        assert!(text.contains("  /src/a.c: fldz R x1, G x1\n"), "{text}");

        let mut alone = VecAudit::default();
        alone.add("/src/a.c", &r, None);
        assert_eq!((alone.counted(), alone.paired), (3, 0));
        let mut same = VecAudit::default();
        same.add("/src/a.c", &g, Some(&g));
        assert_eq!(same.counted(), 0);
    }

    #[test]
    fn the_linked_object_is_read_when_the_compiled_one_is_gone() {
        let dir = std::env::temp_dir().join(format!("rb-vecaudit-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = |name: &str| dir.join(name).to_string_lossy().into_owned();
        std::fs::write(path("memcpy.o"), object(&[("memcpy", SSE)])).unwrap();
        match audit_pair(None, &path("memcpy.o.o")) {
            Pair::Read(r, None) => assert!(r.contains_key("movaps")),
            other => panic!("{other:?}"),
        }
        assert_eq!(audit_pair(None, &path("gone.o")), Pair::Gone);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn only_the_units_without_the_fpu_are_audited() {
        let log = r#"{"started":0,"argv":["cc","-mno-sse","-msoft-float","-c","a.c","-o","a.o"],"compiler":"cc","cwd":"/b/k","inputs":[{"path":"a.c","sha256":""}],"outputs":[{"path":"a.o","sha256":""}],"wall-seconds":0,"exit":0}
{"started":0,"argv":["cc","-c","b.c","-o","b.o"],"compiler":"cc","cwd":"/b/k","inputs":[{"path":"b.c","sha256":""}],"outputs":[{"path":"b.o","sha256":""}],"wall-seconds":0,"exit":0}
"#;
        let (records, _) = harness_shim::record::parse_log(log);
        let keys: Vec<String> = no_fpu_units(&records, "/b").into_keys().collect();
        assert_eq!(keys, ["<root>/k/a.o"]);
    }
}
