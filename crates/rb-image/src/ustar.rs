//! The tar archives of the results disk (plan 12.4).
//!
//! rb-rc writes its results to a raw disk as a tar archive with no file system, so that a broken
//! file system driver in R cannot hide them. rb boot writes an archive with an `rb.conf` to the
//! same disk before the boot, and reads the results back after it. Both ends are here.
//!
//! The writer makes POSIX ustar. The reader takes what the guests' and hosts' `tar` programs
//! write: ustar, pax extended headers (bsdtar on FreeBSD and macOS) and GNU long names (GNU tar
//! on Linux). It stops at the first zero block, so whatever is on the disk after the archive does
//! not count.

use std::path::{Component, Path, PathBuf};

/// The size of a tar block.
pub const BLOCK: usize = 512;

/// What an entry is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A regular file.
    File,
    /// A directory.
    Dir,
    /// Anything else: a link, a device or a FIFO. Its data is empty.
    Other(u8),
}

/// One entry of an archive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The path, relative, with no leading `./` and no trailing `/`.
    pub path: String,
    /// The permission bits.
    pub mode: u32,
    /// What it is.
    pub kind: Kind,
    /// The contents of a file.
    pub data: Vec<u8>,
}

impl Entry {
    /// A regular file.
    #[must_use]
    pub fn file(path: &str, mode: u32, data: impl Into<Vec<u8>>) -> Self {
        Entry {
            path: path.to_string(),
            mode,
            kind: Kind::File,
            data: data.into(),
        }
    }

    /// A directory.
    #[must_use]
    pub fn dir(path: &str) -> Self {
        Entry {
            path: path.to_string(),
            mode: 0o755,
            kind: Kind::Dir,
            data: Vec::new(),
        }
    }
}

/// Write entries as a ustar archive, padded to a whole record of 20 blocks as `tar` does.
pub fn write(entries: &[Entry]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    for e in entries {
        let mut h = [0u8; BLOCK];
        let path = match e.kind {
            Kind::Dir => format!("{}/", e.path.trim_end_matches('/')),
            _ => e.path.clone(),
        };
        let (prefix, name) = split_path(&path)?;
        h[..name.len()].copy_from_slice(name.as_bytes());
        octal(&mut h[100..108], u64::from(e.mode & 0o7777));
        octal(&mut h[108..116], 0);
        octal(&mut h[116..124], 0);
        let size = if e.kind == Kind::File {
            e.data.len()
        } else {
            0
        };
        octal(&mut h[124..136], size as u64);
        octal(&mut h[136..148], 0);
        h[156] = match e.kind {
            Kind::File => b'0',
            Kind::Dir => b'5',
            Kind::Other(t) => t,
        };
        h[257..263].copy_from_slice(b"ustar\0");
        h[263..265].copy_from_slice(b"00");
        h[265..269].copy_from_slice(b"root");
        h[297..302].copy_from_slice(b"wheel");
        h[345..345 + prefix.len()].copy_from_slice(prefix.as_bytes());
        h[148..156].copy_from_slice(b"        ");
        let sum: u32 = h.iter().map(|&b| u32::from(b)).sum();
        h[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        out.extend_from_slice(&h);
        if e.kind == Kind::File {
            out.extend_from_slice(&e.data);
            out.resize(out.len().next_multiple_of(BLOCK), 0);
        }
    }
    out.resize(out.len() + 2 * BLOCK, 0);
    out.resize(out.len().next_multiple_of(20 * BLOCK), 0);
    Ok(out)
}

/// Split a path into the ustar prefix and name fields.
fn split_path(path: &str) -> Result<(&str, &str), String> {
    if path.len() <= 100 {
        return Ok(("", path));
    }
    for (i, c) in path.char_indices() {
        if c == '/' && i <= 155 && path.len() - i - 1 <= 100 && i + 1 < path.len() {
            return Ok((&path[..i], &path[i + 1..]));
        }
    }
    Err(format!("{path}: too long for a ustar header"))
}

/// Write a number as a NUL-terminated octal field.
fn octal(field: &mut [u8], value: u64) {
    let digits = field.len() - 1;
    let s = format!("{value:0digits$o}");
    field[..digits].copy_from_slice(&s.as_bytes()[s.len() - digits..]);
    field[digits] = 0;
}

/// Read a number field: octal with spaces or NULs around it, or base-256 when the high bit of the
/// first byte is set.
fn number(field: &[u8]) -> Result<u64, String> {
    if field.first().is_some_and(|b| b & 0x80 != 0) {
        let mut v: u64 = u64::from(field[0] & 0x7f);
        for &b in &field[1..] {
            v = v.checked_mul(256).ok_or("number field overflows")? + u64::from(b);
        }
        return Ok(v);
    }
    let s: String = field
        .iter()
        .take_while(|&&b| b != 0)
        .map(|&b| b as char)
        .collect();
    let s = s.trim();
    if s.is_empty() {
        return Ok(0);
    }
    u64::from_str_radix(s, 8).map_err(|_| format!("bad number field {s:?}"))
}

/// A NUL-terminated string field.
fn text(field: &[u8]) -> String {
    let end = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    String::from_utf8_lossy(&field[..end]).into_owned()
}

/// Whether a header's checksum is right, taken as unsigned or as signed bytes.
fn checksum_ok(h: &[u8]) -> bool {
    let Ok(want) = number(&h[148..156]) else {
        return false;
    };
    let mut unsigned: u64 = 0;
    let mut signed: i64 = 0;
    for (i, &b) in h.iter().enumerate() {
        let b = if (148..156).contains(&i) { b' ' } else { b };
        unsigned += u64::from(b);
        signed += i64::from(b.cast_signed());
    }
    want == unsigned || i64::try_from(want).is_ok_and(|w| w == signed)
}

/// The value of `key` in a pax extended header.
fn pax(data: &[u8], key: &str) -> Option<String> {
    let mut rest = data;
    while !rest.is_empty() {
        let sp = rest.iter().position(|&b| b == b' ')?;
        let len: usize = std::str::from_utf8(&rest[..sp]).ok()?.parse().ok()?;
        if len <= sp || len > rest.len() {
            return None;
        }
        let record = &rest[sp + 1..len];
        let record = record.strip_suffix(b"\n").unwrap_or(record);
        if let Some(eq) = record.iter().position(|&b| b == b'=')
            && &record[..eq] == key.as_bytes()
        {
            return Some(String::from_utf8_lossy(&record[eq + 1..]).into_owned());
        }
        rest = &rest[len..];
    }
    None
}

/// Make a path relative and plain: no leading `./` or `/`, no trailing `/`.
fn clean(path: &str) -> String {
    let mut p = path.trim_start_matches('/');
    while let Some(rest) = p.strip_prefix("./") {
        p = rest;
    }
    let p = p.trim_end_matches('/');
    if p == "." {
        String::new()
    } else {
        p.to_string()
    }
}

/// Read the entries of an archive. The archive ends at the first zero block or at the end of the
/// bytes. The entry for the top directory (`.` or `./`) is left out.
pub fn read(bytes: &[u8]) -> Result<Vec<Entry>, String> {
    let mut entries = Vec::new();
    let mut at = 0;
    let mut long_name: Option<String> = None;
    let mut pax_path: Option<String> = None;
    let mut pax_size: Option<u64> = None;
    while at + BLOCK <= bytes.len() {
        let h = &bytes[at..at + BLOCK];
        if h.iter().all(|&b| b == 0) {
            break;
        }
        if !checksum_ok(h) {
            return Err(format!("bad tar header checksum at byte {at}"));
        }
        let flag = h[156];
        let size = match pax_size.take() {
            Some(s) if !matches!(flag, b'x' | b'g' | b'L') => s,
            _ => number(&h[124..136])?,
        };
        let size = usize::try_from(size).map_err(|_| "entry too large".to_string())?;
        let start = at + BLOCK;
        let end = start.checked_add(size).ok_or("entry too large")?;
        if end > bytes.len() {
            return Err(format!(
                "entry at byte {at} runs past the end of the archive"
            ));
        }
        let data = &bytes[start..end];
        at = start + size.next_multiple_of(BLOCK);
        match flag {
            b'x' => {
                pax_path = pax(data, "path");
                pax_size = pax(data, "size").and_then(|s| s.parse().ok());
                continue;
            }
            b'g' => continue,
            b'L' => {
                long_name = Some(text(data));
                continue;
            }
            _ => {}
        }
        let mut path = text(&h[..100]);
        if &h[257..262] == b"ustar" {
            let prefix = text(&h[345..500]);
            if !prefix.is_empty() {
                path = format!("{prefix}/{path}");
            }
        }
        if let Some(p) = long_name.take() {
            path = p;
        }
        if let Some(p) = pax_path.take() {
            path = p;
        }
        let kind = match flag {
            b'0' | 0 | b'7' if !path.ends_with('/') => Kind::File,
            b'0' | 0 | b'5' => Kind::Dir,
            t => Kind::Other(t),
        };
        let path = clean(&path);
        if path.is_empty() {
            continue;
        }
        let mode = u32::try_from(number(&h[100..108])? & 0o7777).unwrap_or(0o644);
        let data = if kind == Kind::File {
            data.to_vec()
        } else {
            Vec::new()
        };
        entries.push(Entry {
            path,
            mode,
            kind,
            data,
        });
    }
    Ok(entries)
}

/// Whether a path from an archive stays inside the directory it is unpacked into.
fn safe(path: &str) -> bool {
    Path::new(path)
        .components()
        .all(|c| matches!(c, Component::Normal(_)))
}

/// Read the archive at the start of a disk or a file, reading only as far as the archive goes. A
/// results disk is a gigabyte or more, and the archive on it is usually a few megabytes.
pub fn read_disk(path: &Path) -> Result<Vec<Entry>, String> {
    use std::io::Read as _;
    let err = |e: std::io::Error| format!("{}: {e}", path.display());
    let mut file = std::fs::File::open(path).map_err(err)?;
    let mut bytes = Vec::new();
    let mut pax_size: Option<u64> = None;
    loop {
        let mut h = [0u8; BLOCK];
        match file.read_exact(&mut h) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(e) => return Err(err(e)),
        }
        bytes.extend_from_slice(&h);
        if h.iter().all(|&b| b == 0) || !checksum_ok(&h) {
            break;
        }
        let flag = h[156];
        let size = match pax_size.take() {
            Some(s) if !matches!(flag, b'x' | b'g' | b'L') => s,
            _ => number(&h[124..136])?,
        };
        let size = usize::try_from(size).map_err(|_| "entry too large".to_string())?;
        let start = bytes.len();
        bytes.resize(start + size.next_multiple_of(BLOCK), 0);
        if let Err(e) = file.read_exact(&mut bytes[start..]) {
            if e.kind() == std::io::ErrorKind::UnexpectedEof {
                break;
            }
            return Err(err(e));
        }
        if flag == b'x' {
            pax_size = pax(&bytes[start..start + size], "size").and_then(|s| s.parse().ok());
        }
    }
    read(&bytes)
}

/// Write the files and directories of an archive under `dest`. Other entries are left out, and
/// so is any path that would leave `dest`. Returns the files written.
pub fn unpack(entries: &[Entry], dest: &Path) -> Result<Vec<PathBuf>, String> {
    let mut written = Vec::new();
    std::fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    for e in entries {
        if !safe(&e.path) {
            continue;
        }
        let path = dest.join(&e.path);
        match e.kind {
            Kind::Dir => {
                std::fs::create_dir_all(&path)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
            }
            Kind::File => {
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|err| format!("{}: {err}", parent.display()))?;
                }
                std::fs::write(&path, &e.data)
                    .map_err(|err| format!("{}: {err}", path.display()))?;
                written.push(path);
            }
            Kind::Other(_) => {}
        }
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip() {
        let long = format!("{}/{}", "d".repeat(120), "f".repeat(90));
        let entries = vec![
            Entry::dir("smoke"),
            Entry::file("smoke/counts", 0o644, "5/6\n"),
            Entry::file("empty", 0o600, ""),
            Entry::file(&long, 0o644, vec![7u8; 1000]),
        ];
        let tar = write(&entries).unwrap();
        assert_eq!(tar.len() % (20 * BLOCK), 0);
        assert_eq!(read(&tar).unwrap(), entries);
    }

    #[test]
    fn too_long() {
        let path = "x".repeat(300);
        assert!(write(&[Entry::file(&path, 0o644, "")]).is_err());
    }

    #[test]
    fn stops_at_the_zero_block() {
        let mut disk = write(&[Entry::file("a", 0o644, "new")]).unwrap();
        // An older, longer archive after the end of this one does not count.
        disk.extend(write(&[Entry::file("b", 0o644, "old")]).unwrap());
        let got = read(&disk).unwrap();
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].path, "a");
        assert!(read(&[0u8; 4096]).unwrap().is_empty());
        assert!(read(&[]).unwrap().is_empty());
    }

    #[test]
    fn reads_a_disk_only_to_the_end_of_the_archive() {
        let dir = std::env::temp_dir().join(format!("rb-ustar-disk-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let disk = dir.join("disk");
        let entries = vec![
            Entry::dir("smoke"),
            Entry::file("smoke/counts", 0o644, "5/6\n"),
            Entry::file("big", 0o644, vec![7u8; 3000]),
        ];
        let mut bytes = write(&entries).unwrap();
        bytes.extend_from_slice(b"old results after the end");
        std::fs::write(&disk, &bytes).unwrap();
        let file = std::fs::OpenOptions::new().write(true).open(&disk).unwrap();
        file.set_len(64 << 20).unwrap();
        assert_eq!(read_disk(&disk).unwrap(), entries);
        std::fs::write(&disk, vec![0u8; 4096]).unwrap();
        assert!(read_disk(&disk).unwrap().is_empty());
        std::fs::write(&disk, b"short").unwrap();
        assert!(read_disk(&disk).unwrap().is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn bad_checksum() {
        let mut tar = write(&[Entry::file("a", 0o644, "x")]).unwrap();
        tar[0] = b'b';
        assert!(read(&tar).unwrap_err().contains("checksum"));
    }

    #[test]
    fn pax_and_gnu_names() {
        // A pax header that renames the next entry and gives its size.
        let long = "p/".repeat(80) + "file";
        // A pax record starts with its own length, digits included.
        let record = |k: &str, v: &str| {
            let body = format!(" {k}={v}\n");
            let mut n = body.len() + 1;
            while n.to_string().len() + body.len() != n {
                n = n.to_string().len() + body.len();
            }
            format!("{n}{body}")
        };
        let pax_data = record("path", &long) + &record("size", "3");
        assert_eq!(
            pax(pax_data.as_bytes(), "path").as_deref(),
            Some(long.as_str())
        );
        let mut x = Entry::file("PaxHeader/file", 0o644, pax_data);
        x.kind = Kind::Other(b'x');
        let mut tar = write_raw(&x);
        tar.extend(write_raw(&Entry::file("short", 0o644, "abc")));
        // A GNU long name.
        let mut l = Entry::file("././@LongLink", 0o644, format!("{long}2\0"));
        l.kind = Kind::Other(b'L');
        tar.extend(write_raw(&l));
        tar.extend(write_raw(&Entry::file("short2", 0o644, "de")));
        tar.extend([0u8; 1024]);
        let got = read(&tar).unwrap();
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].path, long);
        assert_eq!(got[0].data, b"abc");
        assert_eq!(got[1].path, format!("{long}2"));
        assert_eq!(got[1].data, b"de");
    }

    /// One entry with its data, as `write` makes it, with no end blocks.
    fn write_raw(e: &Entry) -> Vec<u8> {
        let mut e = e.clone();
        let kind = e.kind;
        if matches!(kind, Kind::Other(_)) {
            e.kind = Kind::File;
        }
        let mut tar = write(&[e.clone()]).unwrap();
        let len = BLOCK + e.data.len().next_multiple_of(BLOCK);
        tar.truncate(len);
        if let Kind::Other(t) = kind {
            tar[156] = t;
            tar[148..156].copy_from_slice(b"        ");
            let sum: u32 = tar[..BLOCK].iter().map(|&b| u32::from(b)).sum();
            tar[148..156].copy_from_slice(format!("{sum:06o}\0 ").as_bytes());
        }
        tar
    }

    #[test]
    fn unpack_keeps_inside() {
        let dest = std::env::temp_dir().join(format!("rb-image-unpack-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dest);
        let entries = vec![
            Entry::file("ok/a", 0o644, "a"),
            Entry::file("../escape", 0o644, "x"),
            Entry::dir("d"),
        ];
        let written = unpack(&entries, &dest).unwrap();
        assert_eq!(written, vec![dest.join("ok/a")]);
        assert!(dest.join("d").is_dir());
        assert!(!dest.parent().unwrap().join("escape").exists());
        std::fs::remove_dir_all(&dest).unwrap();
    }
}
