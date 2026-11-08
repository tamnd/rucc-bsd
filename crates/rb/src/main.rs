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
        "fetch" => {
            let repo = rb::repo::Repo::find()?;
            let report = rb::fetch::run(
                &repo,
                args.target.as_deref(),
                args.get("set"),
                args.has("tarball"),
            )?;
            print!("{report}");
            Ok(ExitCode::SUCCESS)
        }
        "personas" => {
            let repo = rb::repo::Repo::find()?;
            let (report, review) = rb::personas::run(&repo, args.get("arch").unwrap_or("x86_64"))?;
            print!("{report}");
            Ok(if review {
                ExitCode::from(1)
            } else {
                ExitCode::SUCCESS
            })
        }
        "sets" => {
            let repo = rb::repo::Repo::find()?;
            let web = rb_pins::web::Curl::from_env();
            let outcome = rb::sets::run(&repo, &web, args.get("as-of"), args.has("write"))?;
            print!("{}", outcome.report);
            Ok(if outcome.up_to_date || args.has("write") {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            })
        }
        "version" => {
            println!("rb {}", env!("CARGO_PKG_VERSION"));
            Ok(ExitCode::SUCCESS)
        }
        other => Err(format!("unknown command {other}; see rb help")),
    }
}
