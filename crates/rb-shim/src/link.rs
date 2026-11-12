//! The `ld` slot: the reference linker, told to define the bounds of every linker set.
//!
//! FreeBSD's `__MAKE_SET` (`sys/sys/linker_set.h`) declares `__start_set_NAME` and
//! `__stop_set_NAME` weak with `__asm__(".weak ...")`. The integrated assembler of Clang keeps
//! such a symbol as an undefined weak reference, so ld defines it for every set. GNU as drops a
//! weak symbol that nothing uses, and GNU ld defines a `__start_` or `__stop_` symbol only when
//! something refers to it. So a kernel that GCC compiles has the bounds only of the sets that its
//! C code walks itself. `sdt.ko` finds the kernel's probes with `linker_file_lookup_set`, which
//! looks the bounds up by name, so on the G kernel it finds none, and `kldload sdt` panics in
//! `sdt_load_probes_cb` (rucc-bsd#37).
//!
//! For a final link the slot adds `-u __start_set_NAME -u __stop_set_NAME` for each `set_NAME`
//! section of the input objects, which is the reference that the objects of a Clang build make.
//! It adds nothing to a relocatable link (`-r`), whose output keeps its sections for the next link.
//! It adds nothing to a link with `--gc-sections` either, such as that of the boot blocks: there a
//! `-u` is a root that keeps each set section and all the code it points to, while the weak
//! references of a Clang object keep nothing, so the link would pull in commands whose callees
//! are not linked (`gptzfsboot` and `command_errmsg`).
//!
//! For a final link of an arm64 kernel or module, which is the emulation `aarch64elf` and no
//! `-r`, the slot also adds `-z force-bti`. `kern.mk` builds the kernel with
//! `-mbranch-protection=standard`, which defines `__ARM_FEATURE_BTI_DEFAULT`, and `pmap.c` then maps
//! the kernel text guarded. A call through a register into a PLT or IPLT stub must land on a
//! `bti c` there. ld.bfd writes that landing pad only when every input object has the BTI bit in
//! its `.note.gnu.property`, or when it is told `-z force-bti`. Clang writes the note and lld then
//! writes the pads, which is how the stock kernel boots. GCC writes no note on FreeBSD, and rucc
//! writes none to match it, so without the option the IPLT stub of an ifunc such as
//! `sched_idletd` has no pad and the kernel takes a BTI fault when the APs start their idle
//! threads. G and R both link through this slot, so both get the option. ld.bfd then warns once
//! for each input without the note, and the kernel link has no `--fatal-warnings`, so the warnings
//! stay warnings. `kern.mk` asks for `-zbti-report=error` only when the linker is lld.
//!
//! For the final link of a riscv64 kernel, which is the emulation `elf64lriscv` with the dynamic
//! linker `/red/herring` of `SYSTEM_LD_BASECMD` (`sys/conf/kern.pre.mk`), the slot adds
//! `--no-dynamic-linker` after the build's arguments, where the last of the two wins. ld.bfd makes
//! a `PT_PHDR` for the `.interp` section and puts the ELF headers into the first `PT_LOAD`. Since
//! `ldscript.riscv` starts `.text` at `kernbase`, that segment begins one page lower, at
//! `kernbase - 0x1000`. The EFI loader copies the first `PT_LOAD` to the start of its 2 MB aligned
//! staging area (`efi_copyin` in `stand/efi/loader/copy.c`), so `kernbase` lands 4 KB past that
//! boundary. locore maps the kernel with 2 MB pages from the physical address of `kernbase`, so
//! the map is one page off, and the kernel takes an illegal instruction trap at `va` when it turns
//! the MMU on, before it prints a thing. Without `.interp`, ld.bfd writes neither `PT_INTERP` nor
//! `PT_PHDR`, and the first `PT_LOAD` begins at `kernbase`, as in the stock kernel that lld links.
//! The kernel keeps its `PT_DYNAMIC`, and nothing reads the name `/red/herring`. G and R both link
//! through this slot, so both get the option. An image that the firmware boots without the loader
//! starts at `kernel_lma` and does not show the problem.
//!
//! It changes no other argument the build wrote, records nothing and prints nothing.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// The options that make a relocatable link.
const RELOCATABLE: [&str; 4] = ["-r", "--relocatable", "-Ur", "-i"];

/// Whether the command line makes a relocatable object.
#[must_use]
pub fn relocatable(rest: &[String]) -> bool {
    rest.iter().any(|a| RELOCATABLE.contains(&a.as_str()))
}

/// Whether a section name is a linker set whose bounds ld can define: `set_` and a C identifier.
#[must_use]
pub fn is_set(name: &str) -> bool {
    name.len() > 4
        && name.starts_with("set_")
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

/// Whether the link collects unused sections: the last of `--gc-sections` and `--no-gc-sections`
/// wins, and GNU ld takes each with one dash too.
#[must_use]
pub fn gc_sections(rest: &[String]) -> bool {
    rest.iter()
        .rev()
        .find_map(|a| match a.trim_start_matches('-') {
            "gc-sections" if a.starts_with('-') => Some(true),
            "no-gc-sections" if a.starts_with('-') => Some(false),
            _ => None,
        })
        .unwrap_or(false)
}

/// Whether the command line is a final link for arm64, the emulation of FreeBSD's kernel and
/// module links there (`LD_EMULATION_aarch64` in `sys/conf/kern.mk`).
#[must_use]
pub fn arm64_final(rest: &[String]) -> bool {
    let arm64 = rest.iter().enumerate().any(|(i, a)| {
        a == "-maarch64elf" || (a == "-m" && rest.get(i + 1).is_some_and(|e| e == "aarch64elf"))
    });
    arm64 && !relocatable(rest)
}

/// The dynamic linker that `SYSTEM_LD_BASECMD` names for every FreeBSD kernel. Nothing loads it.
const KERNEL_INTERP: &str = "/red/herring";

/// Whether the command line is the final link of a riscv64 kernel: the emulation `elf64lriscv`, no
/// `-r`, and the dynamic linker `/red/herring`, given as `--dynamic-linker` or `-dynamic-linker`,
/// with its value as the next word or after `=`.
#[must_use]
pub fn riscv_kernel(rest: &[String]) -> bool {
    let riscv = rest.iter().enumerate().any(|(i, a)| {
        a == "-melf64lriscv" || (a == "-m" && rest.get(i + 1).is_some_and(|e| e == "elf64lriscv"))
    });
    let interp = rest.iter().enumerate().any(|(i, a)| {
        let a = a.strip_prefix("--").or_else(|| a.strip_prefix('-'));
        match a {
            Some("dynamic-linker") => rest.get(i + 1).is_some_and(|v| v == KERNEL_INTERP),
            Some(a) => a.strip_prefix("dynamic-linker=") == Some(KERNEL_INTERP),
            None => false,
        }
    });
    riscv && interp && !relocatable(rest)
}

/// The arguments for the real linker: `-z force-bti` for a final link for arm64, `-u` for both
/// bounds of every set in the input objects of a final link that does not collect sections, then
/// the build's own arguments, and last `--no-dynamic-linker` for the final link of a riscv64
/// kernel. `sections` reads the section names of an operand, and gives none for a file that is not
/// a relocatable ELF object.
#[must_use]
pub fn args(rest: &[String], cwd: &Path, sections: &dyn Fn(&Path) -> Vec<String>) -> Vec<String> {
    let mut out = head(rest, cwd, sections);
    if riscv_kernel(rest) {
        out.push("--no-dynamic-linker".to_string());
    }
    out
}

/// The arguments of [`args`] up to and with the build's own.
fn head(rest: &[String], cwd: &Path, sections: &dyn Fn(&Path) -> Vec<String>) -> Vec<String> {
    let bti: &[&str] = if arm64_final(rest) {
        &["-z", "force-bti"]
    } else {
        &[]
    };
    if relocatable(rest) || gc_sections(rest) {
        return bti
            .iter()
            .map(|a| (*a).to_string())
            .chain(rest.iter().cloned())
            .collect();
    }
    let sets: BTreeSet<String> = rest
        .iter()
        .filter(|a| !a.starts_with('-') && !a.is_empty())
        .flat_map(|a| sections(&cwd.join(a)))
        .filter(|n| is_set(n))
        .collect();
    let mut out = Vec::with_capacity(bti.len() + rest.len() + 4 * sets.len());
    out.extend(bti.iter().map(|a| (*a).to_string()));
    for set in &sets {
        for bound in ["__start_", "__stop_"] {
            out.push("-u".to_string());
            out.push(format!("{bound}{set}"));
        }
    }
    out.extend(rest.iter().cloned());
    out
}

/// The section names of a relocatable ELF object, or none for any other file.
#[must_use]
pub fn section_names(path: &Path) -> Vec<String> {
    if !path.is_file() {
        return Vec::new();
    }
    File::open(path)
        .and_then(|mut f| read_section_names(&mut f))
        .unwrap_or_default()
}

/// Read the section names of a relocatable ELF object, 32 or 64 bit, of either byte order. Only
/// the header, the section headers and the section name table are read.
///
/// # Errors
///
/// When the file cannot be read. A file that is not a relocatable ELF object gives no names.
pub fn read_section_names<R: Read + Seek>(f: &mut R) -> io::Result<Vec<String>> {
    let mut head = [0u8; 64];
    let got = read_up_to(f, &mut head)?;
    if got < 52 || head[..4] != *b"\x7fELF" {
        return Ok(Vec::new());
    }
    let wide = match head[4] {
        1 => false,
        2 => true,
        _ => return Ok(Vec::new()),
    };
    let big = match head[5] {
        1 => false,
        2 => true,
        _ => return Ok(Vec::new()),
    };
    if wide && got < 64 {
        return Ok(Vec::new());
    }
    let e = Endian { big };
    // ET_REL.
    if e.u16(&head[16..]) != 1 {
        return Ok(Vec::new());
    }
    let (shoff, entsize, shnum, shstrndx) = if wide {
        (
            e.u64(&head[40..]),
            e.u16(&head[58..]),
            e.u16(&head[60..]),
            e.u16(&head[62..]),
        )
    } else {
        (
            u64::from(e.u32(&head[32..])),
            e.u16(&head[46..]),
            e.u16(&head[48..]),
            e.u16(&head[50..]),
        )
    };
    let entsize = usize::from(entsize);
    let need = if wide { 64 } else { 40 };
    if shoff == 0 || entsize < need {
        return Ok(Vec::new());
    }
    let header = |f: &mut R, index: u64| -> io::Result<Vec<u8>> {
        let mut buf = vec![0u8; entsize];
        f.seek(SeekFrom::Start(shoff + index * entsize as u64))?;
        f.read_exact(&mut buf)?;
        Ok(buf)
    };
    // Section 0 holds the real count and name table index when they do not fit the ELF header.
    let zero = header(f, 0)?;
    let count = if shnum == 0 {
        if wide {
            e.u64(&zero[32..])
        } else {
            u64::from(e.u32(&zero[20..]))
        }
    } else {
        u64::from(shnum)
    };
    let names_index = if shstrndx == 0xffff {
        u64::from(e.u32(&zero[if wide { 40 } else { 24 }..]))
    } else {
        u64::from(shstrndx)
    };
    if names_index >= count || count > 1 << 24 {
        return Ok(Vec::new());
    }
    let mut table = vec![0u8; usize::try_from(count).unwrap_or(0) * entsize];
    f.seek(SeekFrom::Start(shoff))?;
    f.read_exact(&mut table)?;
    let place = |h: &[u8]| -> (u32, u64, u64) {
        if wide {
            (e.u32(h), e.u64(&h[24..]), e.u64(&h[32..]))
        } else {
            (
                e.u32(h),
                u64::from(e.u32(&h[16..])),
                u64::from(e.u32(&h[20..])),
            )
        }
    };
    let at = usize::try_from(names_index).unwrap_or(0) * entsize;
    let (_, str_off, str_size) = place(&table[at..at + entsize]);
    if str_size > 1 << 30 {
        return Ok(Vec::new());
    }
    let mut strings = vec![0u8; usize::try_from(str_size).unwrap_or(0)];
    f.seek(SeekFrom::Start(str_off))?;
    f.read_exact(&mut strings)?;
    Ok(table
        .chunks_exact(entsize)
        .skip(1)
        .filter_map(|h| {
            let start = usize::try_from(place(h).0).ok()?;
            let rest = strings.get(start..)?;
            let end = rest.iter().position(|&b| b == 0)?;
            std::str::from_utf8(&rest[..end]).ok().map(String::from)
        })
        .collect())
}

fn read_up_to<R: Read>(f: &mut R, buf: &mut [u8]) -> io::Result<usize> {
    let mut got = 0;
    while got < buf.len() {
        match f.read(&mut buf[got..])? {
            0 => break,
            n => got += n,
        }
    }
    Ok(got)
}

#[derive(Clone, Copy)]
struct Endian {
    big: bool,
}

impl Endian {
    fn u16(self, b: &[u8]) -> u16 {
        let a = [b[0], b[1]];
        if self.big {
            u16::from_be_bytes(a)
        } else {
            u16::from_le_bytes(a)
        }
    }

    fn u32(self, b: &[u8]) -> u32 {
        let a = [b[0], b[1], b[2], b[3]];
        if self.big {
            u32::from_be_bytes(a)
        } else {
            u32::from_le_bytes(a)
        }
    }

    fn u64(self, b: &[u8]) -> u64 {
        let a = [b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7]];
        if self.big {
            u64::from_be_bytes(a)
        } else {
            u64::from_le_bytes(a)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use object::write::{Object, Symbol, SymbolSection};
    use object::{
        Architecture, BinaryFormat, Endianness, SectionKind, SymbolFlags, SymbolKind, SymbolScope,
    };
    use std::io::Cursor;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(String::from).collect()
    }

    /// A relocatable object with a `.text` and the given data sections, as GCC and GNU as write
    /// one that uses `DATA_SET`: the set's section and no symbol for its bounds.
    fn object(arch: Architecture, endian: Endianness, sections: &[&str]) -> Vec<u8> {
        let mut obj = Object::new(BinaryFormat::Elf, arch, endian);
        let text = obj.add_section(Vec::new(), b".text".to_vec(), SectionKind::Text);
        obj.append_section_data(text, &[0xc3], 1);
        for name in sections {
            let id = obj.add_section(Vec::new(), name.as_bytes().to_vec(), SectionKind::Data);
            obj.append_section_data(id, &[0; 8], 8);
        }
        obj.add_symbol(Symbol {
            name: b"f".to_vec(),
            value: 0,
            size: 1,
            kind: SymbolKind::Text,
            scope: SymbolScope::Linkage,
            weak: false,
            section: SymbolSection::Section(text),
            flags: SymbolFlags::None,
        });
        obj.write().unwrap()
    }

    #[test]
    fn the_section_names_of_64_and_32_bit_objects_of_both_orders_are_read() {
        let sets = ["set_sdt_probes_set", "set_sysinit_set"];
        for (arch, endian) in [
            (Architecture::X86_64, Endianness::Little),
            (Architecture::I386, Endianness::Little),
            (Architecture::PowerPc64, Endianness::Big),
            (Architecture::PowerPc, Endianness::Big),
        ] {
            let names = read_section_names(&mut Cursor::new(object(arch, endian, &sets))).unwrap();
            assert!(names.contains(&".text".to_string()), "{arch:?}: {names:?}");
            for set in sets {
                assert!(names.contains(&set.to_string()), "{arch:?}: {names:?}");
            }
        }
    }

    #[test]
    fn a_file_that_is_not_a_relocatable_object_has_no_sections() {
        for data in [
            b"SECTIONS { . = 0; }\n".to_vec(),
            b"!<arch>\n".to_vec(),
            Vec::new(),
        ] {
            assert!(
                read_section_names(&mut Cursor::new(data))
                    .unwrap()
                    .is_empty()
            );
        }
        let mut exec = object(Architecture::X86_64, Endianness::Little, &["set_x"]);
        exec[16] = 2;
        assert!(
            read_section_names(&mut Cursor::new(exec))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn only_set_sections_named_like_c_identifiers_are_sets() {
        assert!(is_set("set_sdt_probes_set"));
        assert!(is_set("set_pcpu"));
        assert!(!is_set("set_"));
        assert!(!is_set(".set_x"));
        assert!(!is_set("set_x.y"));
        assert!(!is_set("data_set"));
    }

    /// rucc-bsd#37: the G kernel had no `__start_set_sdt_probes_set`, because nothing in the
    /// kernel's own objects refers to it. The final link has to ask for it.
    #[test]
    fn a_final_link_asks_for_the_bounds_of_every_set_of_its_objects() {
        let line = "-m elf_x86_64_fbsd -Bdynamic -T ldscript.amd64 --export-dynamic \
                    --dynamic-linker /red/herring -X -o kernel.full kern_sdt.o ip_carp.o vers.o";
        let sections = |p: &Path| -> Vec<String> {
            match p.file_name().and_then(|n| n.to_str()) {
                Some("kern_sdt.o") => words(".text set_sdt_providers_set set_sdt_probes_set"),
                Some("ip_carp.o") => words(".text set_sdt_probes_set set_sdt_tracepoint_set"),
                Some("vers.o") => words(".text .data"),
                _ => Vec::new(),
            }
        };
        let args = args(&words(line), Path::new("/obj/GENERIC"), &sections);
        let added = "-u __start_set_sdt_probes_set -u __stop_set_sdt_probes_set \
                     -u __start_set_sdt_providers_set -u __stop_set_sdt_providers_set \
                     -u __start_set_sdt_tracepoint_set -u __stop_set_sdt_tracepoint_set";
        assert_eq!(args, words(&format!("{added} {line}")));
    }

    #[test]
    fn a_relocatable_link_and_a_question_are_passed_as_they_are() {
        let sections = |_: &Path| words("set_modmetadata_set set_sysinit_set");
        for line in [
            "-m elf_x86_64_fbsd -d -warn-common --build-id=sha1 -r -o sdt.kld sdt.o",
            "-b binary --no-warn-mismatch -r -d -o fw.fwo fw.bin",
            "--relocatable -o x.o a.o",
            "-v",
            "--version",
        ] {
            assert_eq!(args(&words(line), Path::new("/"), &sections), words(line));
        }
    }

    /// The G world: `gptzfsboot` links `zfs_cmd.o` with `--gc-sections`, and a `-u` for
    /// `set_Xcommand_set` kept `command_lszfs` and its reference to `command_errmsg`, which the
    /// boot block does not link.
    #[test]
    fn a_link_that_collects_sections_is_passed_as_it_is() {
        let sections = |_: &Path| words(".text set_Xcommand_set");
        for line in [
            "-m elf_i386_fbsd -static -N --gc-sections -e start -o gptzfsboot.out zfs_cmd.o",
            "-static -gc-sections -o a.out a.o",
            "--no-gc-sections --gc-sections -o a.out a.o",
        ] {
            assert_eq!(args(&words(line), Path::new("/"), &sections), words(line));
        }
        let line = "--gc-sections --no-gc-sections -o a.out a.o";
        assert_eq!(
            args(&words(line), Path::new("/"), &sections),
            words(&format!(
                "-u __start_set_Xcommand_set -u __stop_set_Xcommand_set {line}"
            ))
        );
    }

    #[test]
    fn a_shared_module_link_gets_the_bounds_too() {
        // arm64 and riscv kernel modules are shared objects, and link_elf looks their sets up
        // by symbol, as it does for the kernel.
        let sections = |p: &Path| {
            if p.ends_with("sdt.kld") {
                words("set_modmetadata_set")
            } else {
                Vec::new()
            }
        };
        let line = "-m aarch64elf -Bshareable -znotext -znorelro -d -o sdt.ko sdt.kld";
        assert_eq!(
            args(&words(line), Path::new("/obj"), &sections),
            words(&format!(
                "-z force-bti -u __start_set_modmetadata_set -u __stop_set_modmetadata_set {line}"
            ))
        );
    }

    /// The freebsd-aarch64 R image: the IPLT stub of `sched_idletd` had no `bti c`, and the kernel
    /// took a BTI fault when the APs started their idle threads. A final arm64 link is told to
    /// write the landing pads, and nothing else is.
    #[test]
    fn a_final_arm64_link_writes_bti_landing_pads() {
        let none = |_: &Path| Vec::new();
        let kernel = "-m aarch64elf -Bdynamic -L /src/sys/conf -T ldscript.arm64 --no-relax \
                      --export-dynamic --dynamic-linker /red/herring -X -o kernel.full locore.o vers.o";
        assert_eq!(
            args(&words(kernel), Path::new("/obj"), &none),
            words(&format!("-z force-bti {kernel}"))
        );
        let glued = "-maarch64elf --gc-sections -o a.out a.o";
        assert_eq!(
            args(&words(glued), Path::new("/"), &none),
            words(&format!("-z force-bti {glued}"))
        );
        for line in [
            "-m aarch64elf -d -warn-common --no-relax -r -o sdt.kld sdt.o",
            "-m elf64lriscv -Bshareable -o sdt.ko sdt.kld",
            "-m elf_x86_64_fbsd -Bdynamic -T ldscript.amd64 -o kernel.full vers.o",
            "-m aarch64 -o a.out a.o",
            "--version",
        ] {
            assert_eq!(
                args(&words(line), Path::new("/"), &none),
                words(line),
                "{line}"
            );
        }
    }

    /// The freebsd-riscv64 R image: ld.bfd put the ELF headers into the first `PT_LOAD`, one page
    /// below `kernbase`, the EFI loader put `kernbase` 4 KB past a 2 MB boundary, and the kernel
    /// trapped when it turned the MMU on. The final link of a riscv64 kernel ends with
    /// `--no-dynamic-linker`, and no other link changes.
    #[test]
    fn a_final_riscv64_kernel_link_has_no_interpreter() {
        let none = |_: &Path| Vec::new();
        for kernel in [
            "-m elf64lriscv -Bdynamic -L /src/sys/conf -T ldscript.riscv \
             --defsym=kernel_lma=0x80200000 --export-dynamic --dynamic-linker /red/herring -X \
             -o kernel.full locore.o vers.o",
            "-melf64lriscv -dynamic-linker=/red/herring -o kernel.full locore.o",
            "-m elf64lriscv --dynamic-linker=/red/herring -o kernel.full locore.o",
        ] {
            assert_eq!(
                args(&words(kernel), Path::new("/obj"), &none),
                words(&format!("{kernel} --no-dynamic-linker")),
                "{kernel}"
            );
        }
        for line in [
            "-m elf64lriscv -d -warn-common -r -o sdt.kld sdt.o",
            "-m elf64lriscv -Bshareable -znotext -o sdt.ko sdt.kld",
            "-m elf64lriscv --dynamic-linker /libexec/ld-elf.so.1 -o a.out a.o",
            "-m elf64lriscv --dynamic-linker /red/herring -r -o kernel.o a.o",
            "-m aarch64elf -Bdynamic -T ldscript.arm64 --dynamic-linker /red/herring -o k a.o",
            "-m elf_x86_64_fbsd -Bdynamic -T ldscript.amd64 --dynamic-linker /red/herring -o k a.o",
            "--version",
        ] {
            let got = args(&words(line), Path::new("/"), &none);
            assert!(
                !got.contains(&"--no-dynamic-linker".to_string()),
                "{line}: {got:?}"
            );
        }
    }

    #[test]
    fn section_names_reads_a_file_on_disk() {
        let dir = std::env::temp_dir().join(format!("rb-shim-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.o");
        std::fs::write(
            &path,
            object(
                Architecture::X86_64,
                Endianness::Little,
                &["set_sdt_probes_set"],
            ),
        )
        .unwrap();
        let names = section_names(&path);
        assert!(names.contains(&"set_sdt_probes_set".to_string()));
        assert!(section_names(&dir.join("missing.o")).is_empty());
        assert!(section_names(&dir).is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
