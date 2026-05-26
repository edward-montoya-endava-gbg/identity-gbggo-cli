//! `goctl list-fixtures [--category <c>] [--json]` — enumerate built-in fixtures.

use crate::error::{CliError, CliResult};
use crate::fixtures;
use serde::Serialize;

#[derive(clap::Args, Debug)]
pub struct ListFixturesArgs {
    /// Restrict to one fixture category (e.g. `identity`, `documents`,
    /// `biometrics`).
    #[arg(long)]
    pub category: Option<String>,
    /// Emit as JSON array (`[{"category":"…","name":"…"}, …]`).
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize)]
struct Entry<'a> {
    category: &'a str,
    name: &'a str,
}

pub fn run(args: ListFixturesArgs) -> CliResult<()> {
    let pairs = fixtures::list(args.category.as_deref());

    if args.json {
        let entries: Vec<Entry<'_>> = pairs
            .iter()
            .map(|(c, n)| Entry {
                category: c.as_str(),
                name: n.as_str(),
            })
            .collect();
        let s = serde_json::to_string(&entries)
            .map_err(|e| CliError::config(format!("list-fixtures serialize error: {e}")))?;
        println!("{s}");
    } else {
        // Group by category, sorted alphabetically.
        let mut current: Option<&str> = None;
        for (cat, name) in &pairs {
            if current != Some(cat.as_str()) {
                if current.is_some() {
                    println!();
                }
                println!("{cat}:");
                current = Some(cat.as_str());
            }
            println!("  - {name}");
        }
        if pairs.is_empty() {
            if let Some(c) = &args.category {
                println!("no built-in fixtures registered for category `{c}`");
            } else {
                println!("no built-in fixtures registered");
            }
        }
    }
    Ok(())
}
