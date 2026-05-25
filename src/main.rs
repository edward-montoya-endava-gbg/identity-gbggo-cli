//! `goctl` binary entry point.

use clap::Parser;
use goctl::cli::{dispatch, Cli};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(std::io::stderr)
        .init();

    let raw_args: Vec<String> = std::env::args().collect();
    let mut json_errors = false;
    for a in &raw_args {
        if a == "--json-errors" {
            json_errors = true;
            break;
        }
    }

    if let Err(e) = Cli::enforce_no_duplicate_flags(raw_args.iter().cloned()) {
        return emit_error(e, json_errors);
    }

    let cli = match Cli::try_parse_from(raw_args.iter().cloned()) {
        Ok(c) => c,
        Err(e) => {
            // clap parse / help / version exit. Print to stderr or stdout as clap chose.
            let _ = e.print();
            return ExitCode::from(match e.kind() {
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion => 0,
                _ => 2,
            });
        }
    };

    match dispatch(cli).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => emit_error(e, json_errors),
    }
}

fn emit_error(e: goctl::error::CliError, json_errors: bool) -> ExitCode {
    if json_errors {
        eprintln!("{}", e.to_json_line());
    } else {
        eprintln!("error: {e}");
    }
    e.exit_code()
}
