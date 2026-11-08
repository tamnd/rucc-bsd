//! The `rb` command line. Every command is listed in [`cli::accepted`] and [`cli::USAGE`], and its
//! work is done in the library.

use rb::cli::{self, Args};
use std::process::ExitCode;

fn main() -> ExitCode {
    let words: Vec<String> = std::env::args().skip(1).collect();
    let result = Args::parse(&words, cli::accepted).and_then(|args| run(&args));
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("rb: {message}");
            ExitCode::from(2)
        }
    }
}

/// Run one parsed command.
fn run(args: &Args) -> Result<ExitCode, String> {
    if args.command == "help" || args.has("help") {
        print!("{}", cli::USAGE);
        return Ok(ExitCode::SUCCESS);
    }
    match args.command.as_str() {
        "version" => {
            println!("rb {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command {other}; see rb help")),
    }
}
