//! `goctl list-endpoints [--service <SVC>] [--json] [--disabled-only | --enabled-only] [--api-version <V>]`.

use crate::endpoints::{Catalog, EndpointBody, ListEntry};
use crate::error::{CliError, CliResult};

#[derive(clap::Args, Debug)]
pub struct ListArgs {
    /// Restrict to one service.
    #[arg(long)]
    pub service: Option<String>,
    /// JSON output.
    #[arg(long)]
    pub json: bool,
    /// Only include endpoints that are currently disabled (refuse to run).
    #[arg(long, conflicts_with = "enabled_only")]
    pub disabled_only: bool,
    /// Only include endpoints that are currently enabled.
    #[arg(long, conflicts_with = "disabled_only")]
    pub enabled_only: bool,
    /// Only include versioned endpoints that support the given version.
    #[arg(long = "api-version", value_name = "V")]
    pub api_version: Option<String>,
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
                let fully_disabled = def.is_fully_disabled();
                if args.disabled_only && !fully_disabled {
                    continue;
                }
                if args.enabled_only && fully_disabled {
                    continue;
                }
                match &def.body {
                    EndpointBody::Flat(flat) => {
                        if let Some(_v) = &args.api_version {
                            // Non-versioned endpoints don't match a `--api-version` filter.
                            continue;
                        }
                        entries.push(ListEntry {
                            service: svc,
                            name: &def.name,
                            method: &flat.method,
                            path: &flat.path,
                            description: &def.description,
                            auth: &def.auth,
                            required_vars: &flat.required_vars,
                            disabled: flat.disabled,
                            disabled_reason: flat.disabled_reason.as_deref(),
                            supported_versions: None,
                        });
                    }
                    EndpointBody::Versioned(vb) => {
                        if let Some(v) = &args.api_version {
                            if !vb.supported_versions.contains(v) {
                                continue;
                            }
                        }
                        // Pick a representative version for the top-level
                        // method/path/required_vars columns — prefer the
                        // explicit filter, else the first supported version.
                        let pick = args
                            .api_version
                            .as_deref()
                            .or_else(|| vb.supported_versions.first().map(|s| s.as_str()))
                            .unwrap_or("");
                        let ve = vb.versions.get(pick);
                        let (method, path, required_vars, ve_disabled, ve_reason) =
                            if let Some(ve) = ve {
                                (
                                    ve.method.as_str(),
                                    ve.path.as_str(),
                                    ve.required_vars.as_slice(),
                                    ve.disabled,
                                    ve.disabled_reason.as_deref(),
                                )
                            } else {
                                ("", "", &[] as &[_], false, None)
                            };

                        // For the disabled column on the list output, treat
                        // the endpoint as "disabled" iff every supported
                        // version is disabled (the gate consumer cares about
                        // "can I call this at all"). We still surface the
                        // representative version's `disabled_reason` when one
                        // exists, even if other versions remain enabled — it
                        // hints at what's gated.
                        let entry_disabled = fully_disabled;
                        let entry_reason = if entry_disabled || ve_disabled {
                            ve_reason
                        } else {
                            None
                        };
                        entries.push(ListEntry {
                            service: svc,
                            name: &def.name,
                            method,
                            path,
                            description: &def.description,
                            auth: &def.auth,
                            required_vars,
                            disabled: entry_disabled,
                            disabled_reason: entry_reason,
                            supported_versions: Some(vb.supported_versions.as_slice()),
                        });
                    }
                }
            }
        }
    }

    if args.json {
        let s = serde_json::to_string(&entries)
            .map_err(|e| CliError::config(format!("list-endpoints serialize error: {e}")))?;
        println!("{s}");
    } else {
        for e in &entries {
            let status = if e.disabled { " [disabled]" } else { "" };
            let versions = e
                .supported_versions
                .map(|v| format!(" [{}]", v.join(",")))
                .unwrap_or_default();
            println!(
                "{:<12} {:<7} {}{}{} — {}",
                e.service, e.method, e.name, versions, status, e.description
            );
        }
    }
    Ok(())
}
