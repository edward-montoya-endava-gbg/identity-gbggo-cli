//! `goctl describe <service> <command>` — emit endpoint manifest as JSON or human-readable.

use crate::endpoints::{Catalog, DescribeOutput};
use crate::error::{CliError, CliResult};
#[derive(clap::Args, Debug)]
pub struct DescribeArgs {
    /// Service name (one of: designer, captain-v1, captain-v2, userview).
    pub service: String,
    /// Command name (e.g. `journey-start`).
    pub command: String,
    /// Emit as JSON.
    #[arg(long)]
    pub json: bool,
}

pub fn run(args: DescribeArgs, catalog: &Catalog) -> CliResult<()> {
    let def = catalog.get(&args.service, &args.command).ok_or_else(|| {
        CliError::usage(format!(
            "no command `{}` for service `{}` (services: {})",
            args.command,
            args.service,
            catalog.list_services().join(", ")
        ))
    })?;

    let out = DescribeOutput {
        schema_version: "1",
        service: &args.service,
        name: &def.name,
        method: &def.method,
        path: &def.path,
        description: &def.description,
        auth: &def.auth,
        required_vars: &def.required_vars,
        optional_vars: &def.optional_vars,
        body_template_preview: def.body_template.as_deref(),
        target_url_pattern: format!("<base_url>{}", def.path),
    };

    if args.json {
        let s = serde_json::to_string(&out)
            .map_err(|e| CliError::config(format!("describe serialize error: {e}")))?;
        println!("{s}");
    } else {
        println!("service: {}", args.service);
        println!("name: {}", def.name);
        println!("method: {}", def.method);
        println!("path: {}", def.path);
        println!("description: {}", def.description);
        println!("auth: {:?}", def.auth);
        if !def.required_vars.is_empty() {
            println!("required_vars:");
            for v in &def.required_vars {
                println!(
                    "  - {} ({:?}): {}{}",
                    v.name,
                    v.kind,
                    v.description,
                    v.example
                        .as_ref()
                        .map(|e| format!(" e.g. {e}"))
                        .unwrap_or_default()
                );
            }
        }
        if !def.optional_vars.is_empty() {
            println!("optional_vars:");
            for v in &def.optional_vars {
                println!(
                    "  - {} ({:?}): {}{}",
                    v.name,
                    v.kind,
                    v.description,
                    v.default
                        .as_ref()
                        .map(|d| format!(" (default: {d})"))
                        .unwrap_or_default()
                );
            }
        }
        if let Some(t) = &def.body_template {
            println!("body_template:\n{t}");
        }
    }
    Ok(())
}
