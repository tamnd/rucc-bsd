//! What a guest's serial console says (plan 12.4): rb-rc's markers, the login prompt, a panic and
//! the test that was running when it came, and the results archive when there was no results
//! disk. `rb boot` reads the console with this, and so can anything that has a `console.log`.
//!
//! rb-rc prints these lines, each at the start of a line:
//!
//! | marker | meaning |
//! |---|---|
//! | `RB-READY <uname -a>` | rb-rc started |
//! | `RB-CONF <disk>` | the suites came from the `rb.conf` on the results disk |
//! | `RB-NODISK <disk>` | there is no results disk |
//! | `RB-SUITE <suite>` | a suite starts |
//! | `RB-CHECK <result> <name>` | one smoke check ended |
//! | `RB-CASE <dir>/<program>:<case>` | an ATF test case starts (NetBSD) |
//! | `RB-SUITE-END <suite> <counts>` | a suite ended |
//! | `RB-DONE <suite>=<counts> ... archive=<disk\|console>` | all suites ended |
//! | `RB-POWEROFF-FORCE <seconds>` | the shutdown did not end in time, so rb-rc powers off with no sync |
//! | `RB-LOGIN-REBOOT <securelevel>` | after `rb boot --login` on NetBSD, rb-rc was installed and the guest reboots to run it from `/etc/rc` |
//!
//! On FreeBSD, kyua prints `<dir>/<program>:<case>  ->  ` when a case starts and the result on
//! the same line when it ends. So the last such line names the running case, as `RB-CASE` does.

use serde::{Deserialize, Serialize};

/// Splits console bytes into lines. A prompt such as `login: ` has no newline after it, so the
/// part after the last newline is kept and can be looked at.
#[derive(Debug, Default)]
pub struct Lines {
    partial: Vec<u8>,
}

impl Lines {
    /// Take more bytes, and return the lines they end, without `\r` and `\n`.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        let mut out = Vec::new();
        for &b in bytes {
            if b == b'\n' {
                out.push(text(&self.partial));
                self.partial.clear();
            } else {
                self.partial.push(b);
            }
        }
        out
    }

    /// The text after the last newline.
    #[must_use]
    pub fn partial(&self) -> String {
        text(&self.partial)
    }

    /// The text after the last newline, when the console has ended.
    pub fn finish(&mut self) -> Option<String> {
        let rest = std::mem::take(&mut self.partial);
        (!rest.is_empty()).then(|| text(&rest))
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches('\r')
        .to_string()
}

/// One suite as the console showed it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Suite {
    /// Its name.
    pub name: String,
    /// The counts from `RB-SUITE-END`, such as `5/6`, `pass` or `error`. None while it runs.
    pub result: Option<String>,
}

/// What the console showed.
// Each flag is a separate thing the console said, and any of them can be set.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    /// The `uname -a` of `RB-READY`.
    pub ready: Option<String>,
    /// Whether rb-rc read its `rb.conf` from the results disk.
    pub conf_from_disk: bool,
    /// Whether rb-rc found no results disk.
    pub no_disk: bool,
    /// The suites, in the order they started.
    pub suites: Vec<Suite>,
    /// The smoke checks: the name and the result.
    pub checks: Vec<(String, String)>,
    /// The last test case that started.
    pub case: Option<String>,
    /// The first panic line.
    pub panic: Option<String>,
    /// The test case that was running at the panic.
    pub panic_case: Option<String>,
    /// The suite that was running at the panic.
    pub panic_suite: Option<String>,
    /// Whether the kernel debugger's prompt came.
    pub ddb: bool,
    /// Whether a login prompt came.
    pub login: bool,
    /// The `RB-DONE` line, without the marker.
    pub done: Option<String>,
    /// Where rb-rc says the results are: `disk` or `console`.
    pub archive: Option<String>,
    /// Whether rb-rc powered off with no sync because the shutdown did not end in time.
    #[serde(default)]
    pub forced_poweroff: bool,
    /// Whether rb-rc's `login` script installed rb-rc and rebooted, after `rb boot --login`.
    #[serde(default)]
    pub login_reboot: bool,
    /// The results archive, when rb-rc wrote it to the console.
    #[serde(skip)]
    pub console_tar: Option<Vec<u8>>,
    /// The base64 text of a console archive that has not ended yet.
    #[serde(skip)]
    base64: Option<String>,
}

/// Where a kernel panic message starts in a line: `panic: ` at the start of the line or after a
/// space, or FreeBSD's `Fatal trap`.
fn panic_at(line: &str) -> Option<usize> {
    let mut from = 0;
    while let Some(i) = line[from..].find("panic: ").map(|i| i + from) {
        if i == 0 || line[..i].ends_with(char::is_whitespace) {
            return Some(i);
        }
        from = i + 1;
    }
    line.find("Fatal trap ")
}

/// Whether text ends with the kernel debugger's prompt: `db> ` on FreeBSD, `db{0}> ` on NetBSD.
fn ends_with_ddb(text: &str) -> bool {
    let t = text.trim_end();
    if t.ends_with("db>") {
        return true;
    }
    let Some(open) = t.rfind("db{") else {
        return false;
    };
    let rest = &t[open + 3..];
    rest.strip_suffix("}>")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}

impl Seen {
    /// Take one whole console line into account.
    pub fn line(&mut self, line: &str) {
        let line = line.trim_end_matches('\r');
        if let Some(b64) = &mut self.base64 {
            if line == "====" {
                self.console_tar = decode_base64(b64);
                self.base64 = None;
            } else {
                b64.push_str(line.trim());
            }
            return;
        }
        if line.starts_with("begin-base64 ") {
            self.base64 = Some(String::new());
            return;
        }
        if let Some(rest) = line.strip_prefix("RB-READY") {
            self.ready = Some(rest.trim().to_string());
        } else if line.starts_with("RB-CONF") {
            self.conf_from_disk = true;
        } else if line.starts_with("RB-NODISK") {
            self.no_disk = true;
        } else if let Some(rest) = line.strip_prefix("RB-SUITE-END ") {
            let (name, result) = rest.split_once(' ').unwrap_or((rest, ""));
            match self
                .suites
                .iter_mut()
                .rev()
                .find(|s| s.name == name && s.result.is_none())
            {
                Some(s) => s.result = Some(result.trim().to_string()),
                None => self.suites.push(Suite {
                    name: name.to_string(),
                    result: Some(result.trim().to_string()),
                }),
            }
        } else if let Some(rest) = line.strip_prefix("RB-SUITE ") {
            self.suites.push(Suite {
                name: rest.trim().to_string(),
                result: None,
            });
        } else if let Some(rest) = line.strip_prefix("RB-CHECK ") {
            if let Some((result, name)) = rest.split_once(' ') {
                self.checks
                    .push((name.trim().to_string(), result.to_string()));
            }
        } else if let Some(rest) = line.strip_prefix("RB-CASE ") {
            self.case = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("RB-DONE") {
            let rest = rest.trim();
            self.archive = rest
                .split_whitespace()
                .find_map(|w| w.strip_prefix("archive="))
                .map(str::to_string);
            self.done = Some(rest.to_string());
        } else if line.starts_with("RB-POWEROFF-FORCE") {
            self.forced_poweroff = true;
        } else if line.starts_with("RB-LOGIN-REBOOT") {
            self.login_reboot = true;
        } else {
            self.other(line);
        }
    }

    /// A line that is not a marker: a kyua case, a panic or a prompt.
    fn other(&mut self, line: &str) {
        let mut rest = line;
        if let Some((case, after)) = line.split_once("  ->  ") {
            let case = case.trim();
            if !case.is_empty() && !case.contains(' ') && case.contains(':') {
                self.case = Some(case.to_string());
            }
            rest = after;
        }
        if self.panic.is_none()
            && let Some(at) = panic_at(rest)
        {
            self.panic = Some(rest[at..].trim().to_string());
            self.panic_case.clone_from(&self.case);
            self.panic_suite = self
                .suites
                .iter()
                .rev()
                .find(|s| s.result.is_none())
                .map(|s| s.name.clone());
        }
        self.prompt(line);
    }

    /// Look at the text after the last newline, where a prompt waits.
    pub fn prompt(&mut self, text: &str) {
        if ends_with_ddb(text) {
            self.ddb = true;
            if self.panic.is_none() {
                self.panic_case.clone_from(&self.case);
            }
        }
        if login_prompt(text) {
            self.login = true;
        }
    }

    /// The suite that started last and has not ended.
    #[must_use]
    pub fn running(&self) -> Option<&str> {
        self.suites
            .iter()
            .rev()
            .find(|s| s.result.is_none())
            .map(|s| s.name.as_str())
    }
}

/// Read a console as a whole.
#[must_use]
pub fn read(bytes: &[u8]) -> Seen {
    let mut lines = Lines::default();
    let mut seen = Seen::default();
    for line in lines.push(bytes) {
        seen.line(&line);
    }
    if let Some(rest) = lines.finish() {
        seen.line(&rest);
    }
    seen
}

/// Decode base64, as `uuencode -m` writes it. None if it is not base64.
#[must_use]
pub fn decode_base64(text: &str) -> Option<Vec<u8>> {
    let value = |c: u8| -> Option<u32> {
        match c {
            b'A'..=b'Z' => Some(u32::from(c - b'A')),
            b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
            b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };
    let clean: Vec<u8> = text.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    let body = clean
        .strip_suffix(b"==")
        .or_else(|| clean.strip_suffix(b"="))
        .unwrap_or(&clean);
    let mut out = Vec::with_capacity(body.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0;
    for &c in body {
        acc = (acc << 6) | value(c)?;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(u8::try_from((acc >> bits) & 0xff).unwrap_or(0));
        }
    }
    Some(out)
}

/// Whether the text is a login prompt: `login:` alone or after a space, as getty prints it. The
/// text is often a part of a line, so a word that ends in `login:`, such as the ATF case
/// `t_getlogin:getlogin_r_err` cut after its colon, is not a prompt, and no marker of rb-rc is.
fn login_prompt(text: &str) -> bool {
    let text = text.trim();
    !text.starts_with("RB-") && (text == "login:" || text.ends_with(" login:"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_and_prompts() {
        let mut lines = Lines::default();
        assert_eq!(lines.push(b"a\r\nb"), ["a"]);
        assert_eq!(lines.partial(), "b");
        assert_eq!(lines.push(b"c\nlogin: "), ["bc"]);
        assert_eq!(lines.partial(), "login: ");
        let mut seen = Seen::default();
        seen.prompt(&lines.partial());
        assert!(seen.login && !seen.ddb);
        assert_eq!(lines.finish().as_deref(), Some("login: "));
        assert_eq!(lines.finish(), None);
    }

    #[test]
    fn a_word_that_ends_in_login_is_not_a_prompt() {
        assert!(login_prompt("login: "));
        assert!(login_prompt("netbsd login: "));
        let mut lines = Lines::default();
        let mut seen = Seen::default();
        lines.push(b"RB-CASE lib/libc/sys/t_getlogin:");
        seen.prompt(&lines.partial());
        assert!(!seen.login);
        assert_eq!(
            lines.push(b"getlogin_r_err\r\n"),
            ["RB-CASE lib/libc/sys/t_getlogin:getlogin_r_err"]
        );
        seen.prompt(&lines.partial());
        for text in [
            "t_getlogin:",
            "lib/libc/sys/t_getlogin:",
            "RB-CASE x login:",
        ] {
            seen.prompt(text);
        }
        assert!(!seen.login);
    }

    #[test]
    fn a_whole_run() {
        let console = "\
Copyright (c) 1992-2026 The FreeBSD Project.
RB-READY FreeBSD rb 16.0-CURRENT FreeBSD 16.0-CURRENT amd64
RB-CONF /dev/vtbd1
RB-SUITE boot
RB-SUITE-END boot pass
RB-SUITE smoke
RB-CHECK pass shells
RB-CHECK fail network
RB-CHECK skip package
RB-SMOKE 1/2
RB-SUITE-END smoke 1/2
RB-SUITE tests
sys/kern/pipe:simple  ->  passed  [0.012s]
RB-SUITE-END tests 7000/7100
RB-DONE boot=pass smoke=1/2 tests=7000/7100 archive=disk
";
        let seen = read(console.as_bytes());
        assert_eq!(
            seen.ready.as_deref(),
            Some("FreeBSD rb 16.0-CURRENT FreeBSD 16.0-CURRENT amd64")
        );
        assert!(seen.conf_from_disk && !seen.no_disk);
        let suites: Vec<(&str, Option<&str>)> = seen
            .suites
            .iter()
            .map(|s| (s.name.as_str(), s.result.as_deref()))
            .collect();
        assert_eq!(
            suites,
            [
                ("boot", Some("pass")),
                ("smoke", Some("1/2")),
                ("tests", Some("7000/7100"))
            ]
        );
        assert_eq!(seen.checks.len(), 3);
        assert_eq!(seen.checks[1], ("network".into(), "fail".into()));
        assert_eq!(seen.case.as_deref(), Some("sys/kern/pipe:simple"));
        assert_eq!(
            seen.done.as_deref(),
            Some("boot=pass smoke=1/2 tests=7000/7100 archive=disk")
        );
        assert_eq!(seen.archive.as_deref(), Some("disk"));
        assert!(seen.panic.is_none() && !seen.ddb && !seen.login);
        assert!(!seen.forced_poweroff);
        assert_eq!(seen.running(), None);
    }

    #[test]
    fn a_forced_poweroff() {
        let console = "\
RB-DONE boot=pass tests=5828/6000 archive=disk
Nov 10 01:02:03 rb syslogd[123]: Exiting on signal 15
RB-POWEROFF-FORCE 90
RB-PS   PID  PPID STAT WCHAN   ELAPSED COMMAND
RB-MOUNT /dev/dk0 on / type ffs (local)
";
        let seen = read(console.as_bytes());
        assert!(seen.forced_poweroff);
        assert!(seen.done.is_some() && seen.panic.is_none() && !seen.login);
    }

    #[test]
    fn a_panic_names_the_running_case() {
        // kyua printed the case, and the kernel's lines came after it on the same line.
        let freebsd = "\
RB-READY FreeBSD
RB-SUITE tests
sys/kern/pipe:simple  ->  passed  [0.012s]
sys/vm/mmap:bad  ->  Fatal trap 12: page fault while in kernel mode
panic: page fault
KDB: stack backtrace:
db> ";
        let seen = read(freebsd.as_bytes());
        assert_eq!(
            seen.panic.as_deref(),
            Some("Fatal trap 12: page fault while in kernel mode")
        );
        assert_eq!(seen.panic_case.as_deref(), Some("sys/vm/mmap:bad"));
        assert_eq!(seen.panic_suite.as_deref(), Some("tests"));
        assert!(seen.ddb);
        assert_eq!(seen.running(), Some("tests"));

        let netbsd = "\
RB-SUITE tests
RB-CASE kernel/t_sysv:msg
RB-TESTS-DIR kernel
RB-CASE lib/libc/t_x:y
uvm_fault(0x1, 0x0, 1) -> e
fatal page fault in supervisor mode
panic: trap
cpu0: Begin traceback...
db{0}> ";
        let seen = read(netbsd.as_bytes());
        assert_eq!(seen.panic.as_deref(), Some("panic: trap"));
        assert_eq!(seen.panic_case.as_deref(), Some("lib/libc/t_x:y"));
        assert!(seen.ddb);
    }

    #[test]
    fn panic_words_inside_a_word_are_not_a_panic() {
        let seen = read(b"nopanic: here\nsome kernpanic: text\n");
        assert!(seen.panic.is_none());
        let seen = read(b"[ 1.0] panic: early\n");
        assert_eq!(seen.panic.as_deref(), Some("panic: early"));
    }

    #[test]
    fn ddb_prompts() {
        assert!(ends_with_ddb("db> "));
        assert!(ends_with_ddb("db{0}> "));
        assert!(ends_with_ddb("db{12}>"));
        assert!(!ends_with_ddb("db{x}> "));
        assert!(!ends_with_ddb("mydb>x"));
        let mut seen = Seen {
            case: Some("a:b".into()),
            ..Seen::default()
        };
        seen.prompt("db> ");
        assert!(seen.ddb && seen.panic.is_none());
        assert_eq!(seen.panic_case.as_deref(), Some("a:b"));
    }

    #[test]
    fn the_console_archive() {
        // "hello, rb\n" in base64, as uuencode -m writes it.
        let console = "RB-SUITE boot\nbegin-base64 644 rb.tar\naGVsbG8s\nIHJiCg==\n====\nRB-DONE boot=pass archive=console\n";
        let seen = read(console.as_bytes());
        assert_eq!(seen.console_tar.as_deref(), Some(&b"hello, rb\n"[..]));
        assert_eq!(seen.archive.as_deref(), Some("console"));
        assert_eq!(decode_base64("YQ==").unwrap(), b"a");
        assert_eq!(decode_base64("YWI=").unwrap(), b"ab");
        assert_eq!(decode_base64("YWJj").unwrap(), b"abc");
        assert!(decode_base64("a*b").is_none());
    }
}
