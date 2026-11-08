//! `rb-cc` and its other names: run the compiler the name and `rb-shim.toml` call for, pass its
//! output through, and append one record to `compile.jsonl`.
//!
//! The shim prints nothing of its own unless it cannot run a compiler at all or `RB_TWICE` finds a
//! difference, because `bsd.compiler.mk` and `configure` scripts judge a compiler by what it
//! writes, and a shim that chatters changes the answers it is meant to record.

use harness_shim::args::{self, ProbeRules};
use harness_shim::{env, run};
use rb_shim::config::ShimConfig;
use rb_shim::route::{self, Slot};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().collect();
    let called = argv.first().cloned().unwrap_or_else(|| "rb-cc".to_string());
    let name = Path::new(&called)
        .file_name()
        .map_or_else(|| called.clone(), |n| n.to_string_lossy().into_owned());
    match shim(&argv, &called, &name) {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{name}: {message}");
            ExitCode::from(127)
        }
    }
}

fn shim(argv: &[String], called: &str, name: &str) -> Result<ExitCode, String> {
    let dir = shim_dir(called);
    let config = ShimConfig::load(dir.as_deref(), &|key| std::env::var(key).ok())?;
    let resolved = route::resolve(name, &config).ok_or_else(|| {
        format!("the shim does not know the name {name}: add it to names in rb-shim.toml")
    })?;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let rest = &argv[1..];
    let decision = route::decide(resolved, &config, rest, &cwd, &|p| p.is_file())?;
    if same_file(&decision.compiler, dir.as_deref(), name) {
        return Err(format!(
            "{} is the shim itself; name the real compiler in rb-shim.toml",
            decision.compiler.display()
        ));
    }

    let tmpdir = std::env::var("TMPDIR").ok();
    let rules = ProbeRules {
        component_prefixes: Vec::new(),
        temp_dirs: config.probe_dirs(tmpdir.as_deref()),
    };
    let probe = args::is_probe(rest, &decision.invocation, &rules)
        || resolved.slot == Slot::Cpp && decision.invocation.inputs.is_empty();
    let trace = (config.rucc_trace && decision.rucc && decision.invocation.compiles_c())
        .then(|| run::trace_path("rb-cc"));
    let side = resolved
        .side
        .or(config.side)
        .map(|s| s.letter().to_string());
    let plan = run::Plan {
        argv: argv.to_vec(),
        compiler: decision.compiler,
        args: decision.args,
        added: decision.added,
        cwd,
        invocation: decision.invocation,
        probe,
        trace,
        twice: config.twice,
        retry_refused: decision.delegated.is_some(),
        delegated: decision.delegated,
        env: env::recorded(
            std::env::vars(),
            &[
                "MAKEOBJDIRPREFIX",
                "SRCCONF",
                "__MAKE_CONF",
                "MACHINE",
                "MACHINE_ARCH",
                "TARGET",
                "TARGET_ARCH",
            ],
            &["RB_", "RUCC_"],
        ),
        slot: Some(resolved.slot.name().to_string()),
        side,
        origin: decision.origin.map(String::from),
    };
    let compiler = plan.compiler.clone();
    let record =
        run::execute(plan).map_err(|e| format!("could not run {}: {e}", compiler.display()))?;
    if !config.log.as_os_str().is_empty() {
        // A lost record is a hole in the census, but failing the compile over it would turn a full
        // disk into a compiler bug.
        let _ = record.append_to(&config.log);
    }
    if let Some(twice) = record.twice.as_ref().filter(|t| !t.identical) {
        eprintln!(
            "{name}: RB_TWICE: a second compile wrote different bytes to {}",
            twice.differing.join(", ")
        );
    }
    Ok(run::exit_code(&record))
}

/// The directory the shim was called from, where `rb-shim.toml` lives.
///
/// The name the build used comes first, since every name is a hard link and the operating system
/// may report any one of them as the running executable. A bare name is looked up on `PATH`.
fn shim_dir(called: &str) -> Option<PathBuf> {
    let path = Path::new(called);
    if called.contains('/') {
        return path.parent().map(Path::to_path_buf);
    }
    if let Some(found) = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).find(|dir| dir.join(called).is_file()))
    {
        return Some(found);
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(Path::to_path_buf))
}

/// Whether the compiler is this binary under another name, which would run the shim forever.
fn same_file(compiler: &Path, dir: Option<&Path>, name: &str) -> bool {
    let compiler = if compiler.is_absolute() || compiler.components().count() > 1 {
        compiler.to_path_buf()
    } else {
        match std::env::var_os("PATH")
            .and_then(|paths| std::env::split_paths(&paths).find(|d| d.join(compiler).is_file()))
        {
            Some(dir) => dir.join(compiler),
            None => return false,
        }
    };
    let me = dir.map(|d| d.join(name));
    match (
        compiler.canonicalize(),
        me.and_then(|m| m.canonicalize().ok()),
    ) {
        (Ok(a), Some(b)) if a == b => true,
        (Ok(a), _) => identity(&a) == std::env::current_exe().ok().as_deref().and_then(identity),
        _ => false,
    }
}

/// The device and inode of a file, which hard links share.
#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(path).ok().map(|m| (m.dev(), m.ino()))
}

#[cfg(not(unix))]
fn identity(_path: &Path) -> Option<(u64, u64)> {
    None
}
