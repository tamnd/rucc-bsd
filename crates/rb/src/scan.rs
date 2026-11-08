//! Read compiler features out of one unit's preprocessed text, for `rb demands` (plan 14.5).
//!
//! The scanner works on what `cc -E` printed, so every macro is expanded and every comment is
//! gone. It is a lexer and a few patterns, not a parser. It finds:
//!
//! - attributes, from `__attribute__((...))` and `[[...]]`, with `section` names grouped by
//!   prefix;
//! - builtins, which are the names that start with `__builtin_`, `__sync_` or `__atomic_`;
//! - GNU and C11 keywords, and statement expressions;
//! - inline assembly: the constraints, the constraint modifiers, the operand modifiers of the
//!   template, the clobbers, `asm goto`, `asm volatile`, `asm inline` and asm labels;
//! - file-scope `asm` blocks and the directives in them (07.3);
//! - `#pragma` lines.
//!
//! An assembly unit is read as lines of x86 assembly: `#` starts a comment, `;` separates
//! statements, and the first word of a statement is a directive or a mnemonic. The other rows
//! need their own comment characters before they are scanned.

use std::collections::BTreeMap;

/// The features of one unit: (kind, name) to the number of uses.
pub type Features = BTreeMap<(String, String), usize>;

/// The kinds of feature, as they appear in `demands.toml`.
pub mod kind {
    /// An attribute.
    pub const ATTRIBUTE: &str = "attribute";
    /// A builtin function or type.
    pub const BUILTIN: &str = "builtin";
    /// A keyword or construct outside ISO C99.
    pub const KEYWORD: &str = "keyword";
    /// A use of `asm`.
    pub const ASM: &str = "asm";
    /// An inline assembly constraint.
    pub const CONSTRAINT: &str = "asm constraint";
    /// A constraint modifier: `=`, `+`, `&` or `%`.
    pub const CONSTRAINT_MODIFIER: &str = "asm constraint modifier";
    /// An operand modifier in a template, such as `%k0`.
    pub const OPERAND_MODIFIER: &str = "asm operand modifier";
    /// An inline assembly clobber.
    pub const CLOBBER: &str = "asm clobber";
    /// A directive in a file-scope `asm` block.
    pub const FILE_SCOPE_DIRECTIVE: &str = "file-scope asm directive";
    /// A directive in an assembly unit.
    pub const DIRECTIVE: &str = "asm directive";
    /// An instruction mnemonic in an assembly unit or a file-scope `asm` block.
    pub const MNEMONIC: &str = "mnemonic";
    /// A `#pragma`.
    pub const PRAGMA: &str = "pragma";
    /// A command line flag.
    pub const FLAG: &str = "flag";
}

/// The keywords and spellings outside ISO C99 that a compiler must know.
const KEYWORDS: &[&str] = &[
    "__alignof",
    "__alignof__",
    "__auto_type",
    "__extension__",
    "__imag__",
    "__inline",
    "__inline__",
    "__int128",
    "__label__",
    "__real__",
    "__restrict",
    "__restrict__",
    "__seg_fs",
    "__seg_gs",
    "__signed__",
    "__thread",
    "__typeof",
    "__typeof__",
    "__volatile__",
    "_Alignas",
    "_Alignof",
    "_Atomic",
    "_BitInt",
    "_Complex",
    "_Float128",
    "_Generic",
    "_Noreturn",
    "_Static_assert",
    "_Thread_local",
    "typeof",
];

const ASM: &[&str] = &["asm", "__asm", "__asm__"];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
    Other,
}

fn add(f: &mut Features, kind: &str, name: &str) {
    *f.entry((kind.to_string(), name.to_string())).or_default() += 1;
}

/// The features of a preprocessed C unit.
#[must_use]
pub fn c(text: &str) -> Features {
    let mut f = Features::new();
    let mut code = String::with_capacity(text.len());
    for line in text.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('#') {
            let rest = rest.trim_start();
            if let Some(pragma) = rest.strip_prefix("pragma") {
                add(&mut f, kind::PRAGMA, &pragma_name(pragma));
            }
            code.push('\n');
            continue;
        }
        code.push_str(line);
        code.push('\n');
    }
    let toks = lex(&code);
    walk(&toks, &mut f);
    f
}

/// A pragma by its first words: three for `GCC`, `clang` and `STDC` pragmas, one for the others.
fn pragma_name(rest: &str) -> String {
    let words: Vec<&str> = rest
        .split(|c: char| c.is_whitespace() || c == '(')
        .filter(|w| !w.is_empty())
        .collect();
    let n = match words.first() {
        Some(&("GCC" | "clang" | "STDC")) => 3,
        _ => 1,
    };
    words
        .iter()
        .take(n)
        .map(|w| w.trim_matches('"'))
        .collect::<Vec<_>>()
        .join(" ")
}

fn lex(text: &str) -> Vec<Tok> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        if c.is_ascii_whitespace() {
            i += 1;
        } else if c == b'"' || c == b'\'' {
            let (s, next) = literal(b, i);
            out.push(if c == b'"' { Tok::Str(s) } else { Tok::Other });
            i = next;
        } else if c.is_ascii_alphabetic() || c == b'_' || c == b'$' {
            let start = i;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_' || b[i] == b'$') {
                i += 1;
            }
            let word = &text[start..i];
            if i < b.len()
                && (b[i] == b'"' || b[i] == b'\'')
                && matches!(word, "L" | "u" | "U" | "u8")
            {
                let (s, next) = literal(b, i);
                out.push(if b[i] == b'"' {
                    Tok::Str(s)
                } else {
                    Tok::Other
                });
                i = next;
            } else {
                out.push(Tok::Ident(word.to_string()));
            }
        } else if c.is_ascii_digit() || c == b'.' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
            i += 1;
            while i < b.len()
                && (b[i].is_ascii_alphanumeric()
                    || b[i] == b'.'
                    || b[i] == b'_'
                    || matches!(b[i], b'+' | b'-') && matches!(b[i - 1], b'e' | b'E' | b'p' | b'P'))
            {
                i += 1;
            }
            out.push(Tok::Other);
        } else {
            out.push(Tok::Punct(c as char));
            i += 1;
        }
    }
    out
}

/// Read a string or character literal that starts at `start`, and decode the escapes that
/// matter for assembly text.
#[allow(clippy::many_single_char_names)]
fn literal(b: &[u8], start: usize) -> (String, usize) {
    let quote = b[start];
    let mut i = start + 1;
    let mut s = Vec::new();
    while i < b.len() && b[i] != quote && b[i] != b'\n' {
        if b[i] == b'\\' && i + 1 < b.len() {
            i += 1;
            match b[i] {
                b'n' => s.push(b'\n'),
                b't' => s.push(b'\t'),
                b'0'..=b'7' => {
                    let mut v = 0u32;
                    let mut n = 0;
                    while n < 3 && i < b.len() && (b'0'..=b'7').contains(&b[i]) {
                        v = v * 8 + u32::from(b[i] - b'0');
                        i += 1;
                        n += 1;
                    }
                    s.push(u8::try_from(v & 0xff).unwrap_or(b'?'));
                    continue;
                }
                other => s.push(other),
            }
        } else {
            s.push(b[i]);
        }
        i += 1;
    }
    (String::from_utf8_lossy(&s).into_owned(), i + 1)
}

fn is_punct(t: Option<&Tok>, c: char) -> bool {
    matches!(t, Some(Tok::Punct(p)) if *p == c)
}

/// The index after the group that opens at `open`, which holds `(`, `[` or `{`.
fn close(toks: &[Tok], open: usize) -> usize {
    let mut depth = 0usize;
    let mut i = open;
    while i < toks.len() {
        match &toks[i] {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return i + 1;
                }
            }
            _ => {}
        }
        i += 1;
    }
    toks.len()
}

fn walk(toks: &[Tok], f: &mut Features) {
    let mut depth = 0usize;
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Tok::Punct('{') => depth += 1,
            Tok::Punct('}') => depth = depth.saturating_sub(1),
            Tok::Punct('(') if is_punct(toks.get(i + 1), '{') => {
                add(f, kind::KEYWORD, "statement expression");
            }
            Tok::Punct('[') if is_punct(toks.get(i + 1), '[') => {
                let end = close(toks, i);
                std_attributes(&toks[i + 2..end.saturating_sub(2).max(i + 2)], f);
                i = end;
                continue;
            }
            Tok::Ident(w) if w == "__attribute__" || w == "__attribute" => {
                if is_punct(toks.get(i + 1), '(') {
                    let end = close(toks, i + 1);
                    if is_punct(toks.get(i + 2), '(') {
                        gnu_attributes(&toks[i + 3..end.saturating_sub(2).max(i + 3)], f);
                    }
                    i = end;
                    continue;
                }
            }
            Tok::Ident(w) if ASM.contains(&w.as_str()) => {
                i = asm(toks, i, depth, f);
                continue;
            }
            Tok::Ident(w)
                if w.starts_with("__builtin_")
                    || w.starts_with("__sync_")
                    || w.starts_with("__atomic_") =>
            {
                add(f, kind::BUILTIN, w);
            }
            Tok::Ident(w) if KEYWORDS.contains(&w.as_str()) => add(f, kind::KEYWORD, w),
            _ => {}
        }
        i += 1;
    }
}

/// Split the tokens between the outer parentheses of an attribute list at its top-level commas.
fn items(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Tok::Punct(',') if depth == 0 => {
                out.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&toks[start..]);
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

fn gnu_attributes(toks: &[Tok], f: &mut Features) {
    for item in items(toks) {
        let Some(Tok::Ident(name)) = item.first() else {
            continue;
        };
        let name = name.trim_start_matches("__").trim_end_matches("__");
        if name == "section" {
            let section = item.iter().find_map(|t| match t {
                Tok::Str(s) => Some(s.as_str()),
                _ => None,
            });
            add(
                f,
                kind::ATTRIBUTE,
                &format!("section({})", section_group(section.unwrap_or(""))),
            );
        } else {
            add(f, kind::ATTRIBUTE, name);
        }
    }
}

fn std_attributes(toks: &[Tok], f: &mut Features) {
    for item in items(toks) {
        let mut name = String::new();
        for t in item {
            match t {
                Tok::Ident(w) => name.push_str(w),
                Tok::Punct(':') => name.push(':'),
                _ => break,
            }
        }
        if !name.is_empty() {
            add(f, kind::ATTRIBUTE, &format!("[[{name}]]"));
        }
    }
}

/// The group of a section name: a known prefix, or the name up to its second dot.
#[must_use]
pub fn section_group(name: &str) -> String {
    for prefix in [
        "link_set_",
        "set_",
        ".note.",
        ".init_array",
        ".fini_array",
        ".ctors",
        ".dtors",
    ] {
        if name.starts_with(prefix) {
            return prefix.to_string();
        }
    }
    if let Some(rest) = name.strip_prefix('.') {
        return match rest.find('.') {
            Some(n) => format!(".{}.", &rest[..n]),
            None => name.to_string(),
        };
    }
    if name.is_empty() {
        "?".to_string()
    } else {
        "other".to_string()
    }
}

/// Read one use of `asm` at `at`. Returns the index after it.
fn asm(toks: &[Tok], at: usize, depth: usize, f: &mut Features) -> usize {
    let prev = at.checked_sub(1).and_then(|p| toks.get(p));
    let label = match prev {
        Some(Tok::Ident(w)) => !matches!(w.as_str(), "else" | "do"),
        Some(Tok::Punct(']')) => true,
        Some(Tok::Punct(')')) => depth == 0,
        _ => false,
    };
    let mut i = at + 1;
    let mut qualifiers = Vec::new();
    while let Some(Tok::Ident(w)) = toks.get(i) {
        qualifiers.push(
            w.trim_start_matches("__")
                .trim_end_matches("__")
                .to_string(),
        );
        i += 1;
    }
    if !is_punct(toks.get(i), '(') {
        return at + 1;
    }
    let end = close(toks, i);
    let inner = &toks[i + 1..end.saturating_sub(1).max(i + 1)];
    if label {
        add(
            f,
            kind::ASM,
            if depth == 0 {
                "asm label"
            } else {
                "register asm"
            },
        );
        return end;
    }
    let parts = sections(inner);
    let template: String = parts
        .first()
        .map(|p| {
            p.iter()
                .filter_map(|t| match t {
                    Tok::Str(s) => Some(s.as_str()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    if depth == 0 {
        add(f, kind::ASM, "file-scope asm");
        assembly(&template, kind::FILE_SCOPE_DIRECTIVE, f);
        return end;
    }
    add(f, kind::ASM, "inline asm");
    for q in &qualifiers {
        add(f, kind::ASM, &format!("asm {q}"));
    }
    if parts.len() > 1 {
        operand_modifiers(&template, f);
    }
    for part in parts.iter().skip(1).take(2) {
        for item in items(part) {
            if let Some(Tok::Str(c)) = item.iter().find(|t| matches!(t, Tok::Str(_))) {
                constraint(c, f);
            }
        }
    }
    if let Some(clobbers) = parts.get(3) {
        for t in *clobbers {
            if let Tok::Str(c) = t {
                add(
                    f,
                    kind::CLOBBER,
                    &c.trim_start_matches('%').to_ascii_lowercase(),
                );
            }
        }
    }
    end
}

/// Split the inside of `asm (...)` at its top-level colons.
fn sections(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = 0;
    for (i, t) in toks.iter().enumerate() {
        match t {
            Tok::Punct('(' | '[' | '{') => depth += 1,
            Tok::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Tok::Punct(':') if depth == 0 => {
                out.push(&toks[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(&toks[start..]);
    out
}

/// Record a constraint string: its modifiers, and each constraint letter. x86's two-letter
/// constraints, which start with `Y`, `W`, `B` or `T`, stay whole, and digits are matching
/// constraints.
fn constraint(text: &str, f: &mut Features) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        match c {
            '=' | '+' | '&' | '%' => add(f, kind::CONSTRAINT_MODIFIER, &c.to_string()),
            ',' | ' ' | '*' | '?' | '!' => {}
            '0'..='9' => {
                while i + 1 < chars.len() && chars[i + 1].is_ascii_digit() {
                    i += 1;
                }
                add(f, kind::CONSTRAINT, "matching");
            }
            'Y' | 'W' | 'B' | 'T' if i + 1 < chars.len() => {
                add(f, kind::CONSTRAINT, &format!("{c}{}", chars[i + 1]));
                i += 1;
            }
            _ => add(f, kind::CONSTRAINT, &c.to_string()),
        }
        i += 1;
    }
}

/// Record the operand modifiers of an extended asm template, such as `k` in `%k0`.
#[allow(clippy::many_single_char_names)]
fn operand_modifiers(template: &str, f: &mut Features) {
    let b = template.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i] != b'%' {
            i += 1;
            continue;
        }
        i += 1;
        match b.get(i) {
            Some(b'%') => i += 1,
            Some(b'=') => {
                add(f, kind::OPERAND_MODIFIER, "%=");
                i += 1;
            }
            Some(c) if c.is_ascii_alphabetic() => {
                let start = i;
                while i < b.len() && b[i].is_ascii_alphabetic() {
                    i += 1;
                }
                if matches!(b.get(i), Some(d) if d.is_ascii_digit() || *d == b'[') {
                    add(
                        f,
                        kind::OPERAND_MODIFIER,
                        &format!("%{}", &template[start..i]),
                    );
                }
            }
            _ => {}
        }
    }
}

/// The features of an assembly unit.
#[must_use]
pub fn asm_unit(text: &str) -> Features {
    let mut f = Features::new();
    assembly(text, kind::DIRECTIVE, &mut f);
    f
}

/// Record the directives and mnemonics of assembly text. Directives get `directive_kind`.
fn assembly(text: &str, directive_kind: &str, f: &mut Features) {
    for line in text.lines() {
        if line.trim_start().starts_with('#') {
            continue;
        }
        let line = line.split('#').next().unwrap_or_default();
        for statement in line.split(';') {
            let mut s = statement.trim();
            while let Some(colon) = s.find(':') {
                let label = &s[..colon];
                if !label.is_empty()
                    && label
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$'))
                {
                    s = s[colon + 1..].trim_start();
                } else {
                    break;
                }
            }
            let word = s
                .split(|c: char| c.is_whitespace() || c == ',')
                .next()
                .unwrap_or_default();
            if word.is_empty() || word.starts_with('/') {
                continue;
            }
            if word.starts_with('.') {
                add(f, directive_kind, word);
            } else if word.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                add(f, kind::MNEMONIC, &word.to_ascii_lowercase());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn has(f: &Features, kind: &str, name: &str) -> usize {
        f.get(&(kind.to_string(), name.to_string()))
            .copied()
            .unwrap_or(0)
    }

    #[test]
    fn attributes_builtins_and_keywords_are_found() {
        let text = r#"
# 1 "a.c"
static int x __attribute__((__section__("set_sysinit_set"), __used__)) = 1;
static int y __attribute__((section(".note.tag"))) = 2;
void f(void) __attribute__((noreturn, format(printf, 1, 2)));
[[gnu::packed, deprecated]] struct s { int a; };
int g(int a) { __typeof__(a) b = ({ int c = a; c; }); return __builtin_expect(b, 0) + __atomic_load_n(&a, 5); }
#pragma GCC diagnostic ignored "-Wcast-qual"
#pragma weak foo = bar
"#;
        let f = c(text);
        assert_eq!(has(&f, kind::ATTRIBUTE, "section(set_)"), 1);
        assert_eq!(has(&f, kind::ATTRIBUTE, "section(.note.)"), 1);
        assert_eq!(has(&f, kind::ATTRIBUTE, "used"), 1);
        assert_eq!(has(&f, kind::ATTRIBUTE, "format"), 1);
        assert_eq!(has(&f, kind::ATTRIBUTE, "noreturn"), 1);
        assert_eq!(has(&f, kind::ATTRIBUTE, "[[gnu::packed]]"), 1);
        assert_eq!(has(&f, kind::ATTRIBUTE, "[[deprecated]]"), 1);
        assert_eq!(has(&f, kind::BUILTIN, "__builtin_expect"), 1);
        assert_eq!(has(&f, kind::BUILTIN, "__atomic_load_n"), 1);
        assert_eq!(has(&f, kind::KEYWORD, "__typeof__"), 1);
        assert_eq!(has(&f, kind::KEYWORD, "statement expression"), 1);
        assert_eq!(has(&f, kind::PRAGMA, "GCC diagnostic ignored"), 1);
        assert_eq!(has(&f, kind::PRAGMA, "weak"), 1);
    }

    #[test]
    fn inline_asm_is_taken_apart() {
        let text = r#"
static inline int rd(int *p) {
    int v;
    __asm__ __volatile__("movl %1, %k0\n\tlock; addl $1, %2 # %=" : "=&r"(v), "+m"(*p) : "ir"(1), "0"(v) : "memory", "cc");
    asm goto("jmp %l[out]" :::: out);
out:
    return v;
}
"#;
        let f = c(text);
        assert_eq!(has(&f, kind::ASM, "inline asm"), 2);
        assert_eq!(has(&f, kind::ASM, "asm volatile"), 1);
        assert_eq!(has(&f, kind::ASM, "asm goto"), 1);
        assert_eq!(has(&f, kind::CONSTRAINT_MODIFIER, "="), 1);
        assert_eq!(has(&f, kind::CONSTRAINT_MODIFIER, "&"), 1);
        assert_eq!(has(&f, kind::CONSTRAINT_MODIFIER, "+"), 1);
        assert_eq!(has(&f, kind::CONSTRAINT, "r"), 2);
        assert_eq!(has(&f, kind::CONSTRAINT, "m"), 1);
        assert_eq!(has(&f, kind::CONSTRAINT, "i"), 1);
        assert_eq!(has(&f, kind::CONSTRAINT, "matching"), 1);
        assert_eq!(has(&f, kind::OPERAND_MODIFIER, "%k"), 1);
        assert_eq!(has(&f, kind::OPERAND_MODIFIER, "%l"), 1);
        assert_eq!(has(&f, kind::OPERAND_MODIFIER, "%="), 1);
        assert_eq!(has(&f, kind::CLOBBER, "memory"), 1);
        assert_eq!(has(&f, kind::CLOBBER, "cc"), 1);
    }

    #[test]
    fn file_scope_asm_and_asm_labels_are_told_apart() {
        let text = r#"
__asm__(".pushsection .text\n.globl foo\n.type foo,@function\nfoo: ret\n.popsection");
int open(const char *, int) __asm__("_open");
void f(void) { register long r __asm__("rax"); if (r) __asm__("nop"); }
"#;
        let f = c(text);
        assert_eq!(has(&f, kind::ASM, "file-scope asm"), 1);
        assert_eq!(has(&f, kind::ASM, "asm label"), 1);
        assert_eq!(has(&f, kind::ASM, "register asm"), 1);
        assert_eq!(has(&f, kind::ASM, "inline asm"), 1);
        assert_eq!(has(&f, kind::FILE_SCOPE_DIRECTIVE, ".pushsection"), 1);
        assert_eq!(has(&f, kind::FILE_SCOPE_DIRECTIVE, ".globl"), 1);
        assert_eq!(has(&f, kind::FILE_SCOPE_DIRECTIVE, ".popsection"), 1);
        assert_eq!(has(&f, kind::MNEMONIC, "ret"), 1);
    }

    #[test]
    fn an_assembly_unit_gives_directives_and_mnemonics() {
        let text = "# 1 \"a.S\"\n\t.text\nENTRY: .p2align 4 ; movq %rdi, %rax # copy\n1: RET\n\t.section .note.GNU-stack,\"\",%progbits\n";
        let f = asm_unit(text);
        assert_eq!(has(&f, kind::DIRECTIVE, ".text"), 1);
        assert_eq!(has(&f, kind::DIRECTIVE, ".p2align"), 1);
        assert_eq!(has(&f, kind::DIRECTIVE, ".section"), 1);
        assert_eq!(has(&f, kind::MNEMONIC, "movq"), 1);
        assert_eq!(has(&f, kind::MNEMONIC, "ret"), 1);
        assert_eq!(f.len(), 5);
    }

    #[test]
    fn section_names_are_grouped() {
        assert_eq!(section_group("link_set_modules"), "link_set_");
        assert_eq!(section_group("set_sysctl_set"), "set_");
        assert_eq!(section_group(".text.startup"), ".text.");
        assert_eq!(section_group(".init_array.00099"), ".init_array");
        assert_eq!(section_group(".data"), ".data");
        assert_eq!(section_group("mine"), "other");
    }
}
