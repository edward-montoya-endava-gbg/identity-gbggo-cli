//! `goctl describe-fixture <category> <name> [--json]` — resolve a fixture and
//! emit it as pretty JSON (post `@file:` inlining). The content IS the
//! contract; `--json` is accepted for symmetry but the default is also JSON.

use crate::error::{CliError, CliResult};
use crate::fixtures;

#[derive(clap::Args, Debug)]
pub struct DescribeFixtureArgs {
    /// Fixture category (e.g. `identity`).
    pub category: String,
    /// Fixture name (built-in like `testdata-v1`, or a filesystem path).
    pub name: String,
    /// Accepted for symmetry — output is JSON either way.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: DescribeFixtureArgs) -> CliResult<()> {
    // `--json` is accepted but a no-op: fixture contents ARE the JSON contract.
    let _ = args.json;
    let value = fixtures::resolve(&args.category, &args.name)?;
    let pretty = serde_json::to_string_pretty(&value)
        .map_err(|e| CliError::config(format!("describe-fixture serialize error: {e}")))?;
    println!("{pretty}");
    Ok(())
}
