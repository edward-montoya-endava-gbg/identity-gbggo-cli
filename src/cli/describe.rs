//! `goctl describe <service> <command>` — emit endpoint manifest as JSON or human-readable.

use crate::endpoints::{Catalog, DescribeOutput, DescribeVersion, EndpointBody};
use crate::error::{CliError, CliResult};
use std::collections::BTreeMap;

#[derive(clap::Args, Debug)]
pub struct DescribeArgs {
    /// Service name (one of: designer, captain, userview).
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

    let out: DescribeOutput<'_> = match &def.body {
        EndpointBody::Flat(flat) => DescribeOutput::Flat {
            schema_version: "1",
            service: &args.service,
            name: &def.name,
            method: &flat.method,
            path: &flat.path,
            description: &def.description,
            auth: &def.auth,
            required_vars: &flat.required_vars,
            optional_vars: &flat.optional_vars,
            body_template_preview: flat.body_template.as_deref(),
            target_url_pattern: format!("<base_url>{}", flat.path),
            disabled: flat.disabled,
            disabled_reason: flat.disabled_reason.as_deref(),
        },
        EndpointBody::Versioned(vb) => {
            let mut versions_out: BTreeMap<String, DescribeVersion<'_>> = BTreeMap::new();
            for (v, ve) in &vb.versions {
                versions_out.insert(
                    v.clone(),
                    DescribeVersion {
                        method: &ve.method,
                        path: &ve.path,
                        required_vars: &ve.required_vars,
                        optional_vars: &ve.optional_vars,
                        body_template: ve.body_template.as_deref(),
                        disabled: ve.disabled,
                        disabled_reason: ve.disabled_reason.as_deref(),
                        deprecated: ve.deprecated.as_deref(),
                        target_url_pattern: format!("<base_url:{}>{}", v, ve.path),
                    },
                );
            }
            DescribeOutput::Versioned {
                schema_version: "1",
                service: &args.service,
                name: &def.name,
                description: &def.description,
                auth: &def.auth,
                supported_versions: &vb.supported_versions,
                versions: versions_out,
            }
        }
    };

    if args.json {
        let s = serde_json::to_string(&out)
            .map_err(|e| CliError::config(format!("describe serialize error: {e}")))?;
        println!("{s}");
    } else {
        print_human(&args.service, def);
    }
    Ok(())
}

fn print_human(service: &str, def: &crate::endpoints::EndpointDef) {
    println!("service: {service}");
    println!("name: {}", def.name);
    println!("description: {}", def.description);
    println!("auth: {:?}", def.auth);
    match &def.body {
        EndpointBody::Flat(flat) => {
            if flat.disabled {
                let reason = flat.disabled_reason.as_deref().unwrap_or("disabled");
                println!("Status: disabled ({reason})");
            } else {
                println!("Status: enabled");
            }
            println!("method: {}", flat.method);
            println!("path: {}", flat.path);
            print_vars(&flat.required_vars, &flat.optional_vars);
            if let Some(t) = &flat.body_template {
                println!("body_template:\n{t}");
            }
        }
        EndpointBody::Versioned(vb) => {
            println!("supported_versions: {}", vb.supported_versions.join(", "));
            for (v, ve) in &vb.versions {
                println!("--- version {v} ---");
                if ve.disabled {
                    let reason = ve.disabled_reason.as_deref().unwrap_or("disabled");
                    println!("  Status: disabled ({reason})");
                } else if let Some(d) = &ve.deprecated {
                    println!("  Status: enabled (deprecated: {d})");
                } else {
                    println!("  Status: enabled");
                }
                println!("  method: {}", ve.method);
                println!("  path: {}", ve.path);
                if !ve.required_vars.is_empty() {
                    println!("  required_vars:");
                    for vr in &ve.required_vars {
                        println!(
                            "    - {} ({:?}): {}{}",
                            vr.name,
                            vr.kind,
                            vr.description,
                            vr.example
                                .as_ref()
                                .map(|e| format!(" e.g. {e}"))
                                .unwrap_or_default()
                        );
                    }
                }
                if !ve.optional_vars.is_empty() {
                    println!("  optional_vars:");
                    for vr in &ve.optional_vars {
                        println!(
                            "    - {} ({:?}): {}{}",
                            vr.name,
                            vr.kind,
                            vr.description,
                            vr.default
                                .as_ref()
                                .map(|d| format!(" (default: {d})"))
                                .unwrap_or_default()
                        );
                    }
                }
                if let Some(t) = &ve.body_template {
                    println!("  body_template:\n{t}");
                }
            }
        }
    }
}

fn print_vars(
    required: &[crate::endpoints::RequiredVar],
    optional: &[crate::endpoints::OptionalVar],
) {
    if !required.is_empty() {
        println!("required_vars:");
        for v in required {
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
    if !optional.is_empty() {
        println!("optional_vars:");
        for v in optional {
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
}
