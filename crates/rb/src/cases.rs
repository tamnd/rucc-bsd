//! The per-case results of one boot, read from the files rb-rc saved (plan 12.2, 12.5).
//!
//! The results directory of `rb boot` holds what rb-rc wrote. The cases come from three places:
//! - the boot suite is one case, `boot`, from the outcome of `rb boot`;
//! - the smoke suite has one case per check, from `smoke/results`;
//! - the tests suite has one case per test case: from `tests/kyua.log` on FreeBSD, and from
//!   `tests/atf/DIR.tps` on NetBSD.
//!
//! A test case is named `DIR/PROGRAM:CASE`, where `DIR/PROGRAM` is the test program's path under
//! `/usr/tests`. This is the name kyua prints, and the name rb-rc prints in its `RB-CASE` lines.
//!
//! The results keep the names the tools give them: `passed`, `failed`, `skipped`, `broken`, and
//! the `expected_*` results. The smoke results `pass`, `fail` and `skip` become `passed`, `failed`
//! and `skipped`. A case that started and did not end, because its test program crashed, is
//! `broken`. Only `passed` counts as a pass.

use std::collections::BTreeMap;
use std::path::Path;

/// The result of one case in one run.
#[derive(Debug, Clone, PartialEq)]
pub struct Case {
    /// `boot`, `smoke` or `tests`.
    pub suite: String,
    /// The name, unique within the suite.
    pub name: String,
    /// The result, such as `passed` or `failed`.
    pub result: String,
    /// How long it ran, where the tool says.
    pub seconds: Option<f64>,
}

impl Case {
    fn new(suite: &str, name: &str, result: &str, seconds: Option<f64>) -> Self {
        Case {
            suite: suite.to_string(),
            name: name.to_string(),
            result: result.to_string(),
            seconds,
        }
    }
}

/// The smoke checks, from the lines `RESULT NAME` of `smoke/results`.
#[must_use]
pub fn smoke(text: &str) -> Vec<Case> {
    text.lines()
        .filter_map(|line| {
            let (result, name) = line.trim().split_once(' ')?;
            let result = match result {
                "pass" => "passed",
                "fail" => "failed",
                "skip" => "skipped",
                other => other,
            };
            Some(Case::new("smoke", name.trim(), result, None))
        })
        .collect()
}

/// The test cases in the output of `kyua test`. Each one is a line such as
/// `sys/kern/x_test:y  ->  failed: the reason  [0.012s]`. The other lines do not count.
#[must_use]
pub fn kyua_log(text: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    for line in text.lines() {
        let Some((name, rest)) = line.split_once("  ->  ") else {
            continue;
        };
        let name = name.trim();
        if name.is_empty() || name.contains(' ') || !name.contains(':') {
            continue;
        }
        let result: String = rest
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        if result.is_empty() {
            continue;
        }
        let seconds = rest
            .trim_end()
            .strip_suffix("s]")
            .and_then(|r| r.rsplit_once('['))
            .and_then(|(_, n)| n.parse().ok());
        cases.push(Case::new("tests", name, &result, seconds));
    }
    cases
}

/// The test cases in the output of `atf-run` in the top-level directory `dir` (a `.tps` file).
/// The program on a `tp-start` line is its path under `dir`.
#[must_use]
pub fn tps(text: &str, dir: &str) -> Vec<Case> {
    let mut cases = Vec::new();
    let mut program = String::new();
    // The cases of the current program that started and have not ended, with their start times.
    let mut open: Vec<(String, Option<f64>)> = Vec::new();
    let name = |program: &str, case: &str| format!("{dir}/{program}:{case}");
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("tp-start: ") {
            program = rest.split(", ").nth(1).unwrap_or("?").to_string();
            open.clear();
        } else if let Some(rest) = line.strip_prefix("tc-start: ") {
            if let Some((ts, case)) = rest.split_once(", ") {
                open.push((case.to_string(), ts.parse().ok()));
            }
        } else if let Some(rest) = line.strip_prefix("tc-end: ") {
            let parts: Vec<&str> = rest.splitn(4, ", ").collect();
            if parts.len() < 3 {
                continue;
            }
            let start = open
                .iter()
                .position(|(c, _)| c == parts[1])
                .map(|i| open.remove(i).1);
            let seconds = match (parts[0].parse::<f64>().ok(), start.flatten()) {
                (Some(end), Some(start)) => Some(((end - start) * 100.0).round() / 100.0),
                _ => None,
            };
            cases.push(Case::new(
                "tests",
                &name(&program, parts[1]),
                parts[2],
                seconds,
            ));
        } else if line.starts_with("tp-end: ") {
            for (case, _) in open.drain(..) {
                cases.push(Case::new("tests", &name(&program, &case), "broken", None));
            }
        }
    }
    for (case, _) in open {
        cases.push(Case::new("tests", &name(&program, &case), "broken", None));
    }
    cases
}

/// Every case of one run. `boot` is the result of the boot case: `passed` when rb boot passed,
/// else what rb boot said, such as `panic` or `timeout`. `results` is the directory rb boot
/// wrote the guest's results to, and it may be missing.
pub fn read_run(boot: &str, results: &Path) -> Result<Vec<Case>, String> {
    let read = |rel: &str| -> Result<Option<String>, String> {
        let path = results.join(rel);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Some(String::from_utf8_lossy(&bytes).into_owned())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    };
    let mut cases = vec![Case::new("boot", "boot", boot, None)];
    if let Some(text) = read("smoke/results")? {
        cases.extend(smoke(&text));
    }
    if let Some(text) = read("tests/kyua.log")? {
        cases.extend(kyua_log(&text));
    }
    let atf = results.join("tests/atf");
    if atf.is_dir() {
        let mut files: Vec<_> = std::fs::read_dir(&atf)
            .map_err(|e| format!("{}: {e}", atf.display()))?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "tps"))
            .collect();
        files.sort();
        for file in files {
            let dir = file.file_stem().unwrap_or_default().to_string_lossy();
            let text = std::fs::read(&file).map_err(|e| format!("{}: {e}", file.display()))?;
            cases.extend(tps(&String::from_utf8_lossy(&text), &dir));
        }
    }
    Ok(cases)
}

/// How many cases of each suite had each result.
#[must_use]
pub fn counts(cases: &[Case]) -> BTreeMap<String, BTreeMap<String, u64>> {
    let mut out: BTreeMap<String, BTreeMap<String, u64>> = BTreeMap::new();
    for c in cases {
        *out.entry(c.suite.clone())
            .or_default()
            .entry(c.result.clone())
            .or_default() += 1;
    }
    out
}

/// A kernel message with the parts that change from boot to boot replaced by `#`: the time
/// stamp at the start of a NetBSD line, hex numbers, numbers with a decimal point, and numbers of
/// three digits or more. Small numbers stay, because they name devices such as `vtnet0`.
#[must_use]
pub fn kernel_line(line: &str) -> String {
    let mut line = line.trim_end();
    if let Some(rest) = line.strip_prefix('[')
        && let Some((stamp, after)) = rest.split_once(']')
        && stamp.trim().chars().all(|c| c.is_ascii_digit() || c == '.')
    {
        line = after.trim_start();
    }
    let chars: Vec<char> = line.chars().collect();
    let mut out = String::with_capacity(line.len());
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let word_start = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if c == '0' && word_start && chars.get(i + 1).is_some_and(|&x| x == 'x' || x == 'X') {
            let mut j = i + 2;
            while j < chars.len() && chars[j].is_ascii_hexdigit() {
                j += 1;
            }
            if j > i + 2 {
                out.push_str("0x#");
                i = j;
                continue;
            }
        }
        if c.is_ascii_digit() && (i == 0 || !chars[i - 1].is_ascii_digit()) {
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }
            let number: String = chars[i..j].iter().collect();
            let number = number.trim_end_matches('.');
            let digits = number.chars().filter(char::is_ascii_digit).count();
            let end = i + number.chars().count();
            if number.contains('.') || digits >= 3 {
                out.push('#');
            } else {
                out.push_str(number);
            }
            i = end;
            continue;
        }
        out.push(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn short(cases: &[Case]) -> Vec<(String, String, Option<f64>)> {
        cases
            .iter()
            .map(|c| (c.name.clone(), c.result.clone(), c.seconds))
            .collect()
    }

    #[test]
    fn smoke_results() {
        let cases = smoke("pass shells\nfail network\nskip package\n\n");
        assert_eq!(cases.len(), 3);
        assert_eq!(cases[1].suite, "smoke");
        assert_eq!(cases[1].name, "network");
        assert_eq!(cases[1].result, "failed");
        assert_eq!(cases[2].result, "skipped");
    }

    #[test]
    fn kyua_lines() {
        let log = "\
sys/kern/ptrace_test:ptrace__a  ->  passed  [0.012s]
sys/kern/ptrace_test:ptrace__b  ->  failed: ptrace.c:10: x == 0 not met  [1.500s]
lib/libc/ssp/ssp_test:vsnprintf  ->  skipped: Required configuration property not set  [0.000s]
sys/net/if_x:y  ->  broken: Test case body timed out  [300.017s]
bin/sh/errors:bad_redirect  ->  expected_failure: PR 12345  [0.020s]

Results file id is usr_tests.20261109-070000-000000
Results saved to /var/rb/out/tests/kyua.db

5/1200 passed (1 broken, 1 failed, 1 skipped)
";
        let cases = kyua_log(log);
        assert_eq!(
            short(&cases),
            [
                (
                    "sys/kern/ptrace_test:ptrace__a".into(),
                    "passed".into(),
                    Some(0.012)
                ),
                (
                    "sys/kern/ptrace_test:ptrace__b".into(),
                    "failed".into(),
                    Some(1.5)
                ),
                (
                    "lib/libc/ssp/ssp_test:vsnprintf".into(),
                    "skipped".into(),
                    Some(0.0)
                ),
                ("sys/net/if_x:y".into(), "broken".into(), Some(300.017)),
                (
                    "bin/sh/errors:bad_redirect".into(),
                    "expected_failure".into(),
                    Some(0.02)
                ),
            ]
        );
    }

    #[test]
    fn atf_tps() {
        let text = "\
Content-Type: application/X-atf-tps; version=\"3\"

info: atf.version, Automated Testing Framework 0.20
tps-count: 3
tp-start: 1762671600.000000, t_a, 2
tc-start: 1762671600.100000, one
tc-so:out
tc-end: 1762671600.350000, one, passed
tc-start: 1762671600.400000, two
tc-end: 1762671601.400000, two, failed, the reason, with a comma
tp-end: 1762671601.500000, t_a
tp-start: 1762671602.000000, sub/t_b, 2
tc-start: 1762671602.000000, first
tc-end: 1762671602.000000, first, expected_failure, PR 1
tc-start: 1762671602.100000, second
tp-end: 1762671603.000000, sub/t_b, Test program received signal 11
tp-start: 1762671604.000000, t_c, 1
tc-start: 1762671604.000000, cut
";
        let cases = tps(text, "lib");
        assert_eq!(
            short(&cases),
            [
                ("lib/t_a:one".into(), "passed".into(), Some(0.25)),
                ("lib/t_a:two".into(), "failed".into(), Some(1.0)),
                (
                    "lib/sub/t_b:first".into(),
                    "expected_failure".into(),
                    Some(0.0)
                ),
                ("lib/sub/t_b:second".into(), "broken".into(), None),
                ("lib/t_c:cut".into(), "broken".into(), None),
            ]
        );
    }

    #[test]
    fn a_run_from_its_files() {
        let dir = std::env::temp_dir().join(format!("rb-cases-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("smoke")).unwrap();
        std::fs::create_dir_all(dir.join("tests/atf")).unwrap();
        std::fs::write(dir.join("smoke/results"), "pass shells\n").unwrap();
        std::fs::write(
            dir.join("tests/atf/bin.tps"),
            "tp-start: 1.0, t_sh, 1\ntc-start: 1.0, a\ntc-end: 2.0, a, passed\n",
        )
        .unwrap();
        std::fs::write(dir.join("tests/atf/status"), "bin 1\n").unwrap();
        let cases = read_run("passed", &dir).unwrap();
        let names: Vec<_> = cases
            .iter()
            .map(|c| format!("{}/{}", c.suite, c.name))
            .collect();
        assert_eq!(names, ["boot/boot", "smoke/shells", "tests/bin/t_sh:a"]);
        let counts = counts(&cases);
        assert_eq!(counts["tests"]["passed"], 1);
        // A run with no results has only its boot case.
        let cases = read_run("panic", &dir.join("none")).unwrap();
        assert_eq!(short(&cases), [("boot".into(), "panic".into(), None)]);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn kernel_lines() {
        assert_eq!(
            kernel_line("[   1.0000030] vioif0: Ethernet address 52:54:00:12:34:56"),
            "vioif0: Ethernet address 52:54:00:12:34:56"
        );
        assert_eq!(
            kernel_line("Timecounter \"TSC\" frequency 3591234567 Hz quality 1000"),
            "Timecounter \"TSC\" frequency # Hz quality #"
        );
        assert_eq!(
            kernel_line("vtbd0: 10240MB (20971520 512 byte sectors)"),
            "vtbd0: #MB (# # byte sectors)"
        );
        assert_eq!(
            kernel_line("cpu0 at mainbus0 apid 0 at 0xfee00000, 2.50 GHz."),
            "cpu0 at mainbus0 apid 0 at 0x#, # GHz."
        );
        assert_eq!(kernel_line("uhub0: 4 ports"), "uhub0: 4 ports");
    }
}
