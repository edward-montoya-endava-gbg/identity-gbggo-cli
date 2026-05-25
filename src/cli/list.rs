//! `goctl list-endpoints [--service <SVC>] [--json]`.

use crate::endpoints::{Catalog, ListEntry};
use crate::error::{CliError, CliResult};
#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Restrict to one service.
    #[arg(long)]
    pub service: Option<String>,
    /// JSON output.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: ListArgs, catalog: &Catalog) -> CliResult<()> {
    let services: Vec<&str> = match &args.service {
        Some(s) => {
            if !catalog.services.contains_key(s) {
                return Err(CliError::usage(format!(
                    "service `{s}` not found (services: {})",
                    catalog.list_services().join(", ")
                )));
            }
            vec![s.as_str()]
        }
        None => catalog.list_services(),
    };

    let mut entries: Vec<ListEntry<'_>> = Vec::new();
    for svc in &services {
        if let Some(iter) = catalog.endpoints_for(svc) {
            for def in iter {
                entries.push(ListEntry {
                    service: svc,
                    name: &def.name,
                    method: &def.method,
                    path: &def.path,
                    description: &def.description,
                    auth: &def.auth,
                    required_vars: &def.required_vars,
                });
            }
        }
    }

    if args.json {
        let s = serde_json::to_string(&entries)
            .map_err(|e| CliError::config(format!("list-endpoints serialize error: {e}")))?;
        println!("{s}");
    } else {
        for e in &entries {
            println!(
                "{:<14} {:<7} {} — {}",
                e.service, e.method, e.name, e.description
            );
        }
    }
    Ok(())
}
