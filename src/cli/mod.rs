//! CLI surface — root parser, global flags, dispatch.

pub mod describe;
pub mod list;

use crate::auth::bearer;
use crate::config::RegionsConfig;
use crate::endpoints::Catalog;
use crate::error::{CliError, CliResult};
use crate::exec::ExecInputs;
use clap::{ArgAction, Parser, Subcommand};
use std::collections::BTreeMap;
use std::path::PathBuf;

/// `goctl` — GBG GO API CLI.
///
/// LLM agents: use `goctl list-endpoints --json` and
/// `goctl describe <service> <command> --json` to enumerate the full command grammar.
/// Outputs are versioned via `schema_version: "1"`. Errors emitted with
/// `--json-errors` follow `{schema_version, exit_code, kind, message, context}`.
#[derive(Parser, Debug)]
#[command(
    name = "goctl",
    bin_name = "goctl",
    version,
    about = "GBG GO API CLI — Designer, Captain v1/v2, UserView.",
    long_about = "GBG GO API CLI — Designer, Captain v1/v2, UserView.\n\n\
For LLM agents: every command is discoverable via `goctl list-endpoints` and\n\
`goctl describe <service> <command>`. The JSON contract is versioned\n\
(schema_version: \"1\"). Errors can be emitted as single-line JSON on stderr\n\
with `--json-errors`."
)]
pub struct Cli {
    /// Target environment (dev | demo | prod).
    #[arg(long, global = true, value_name = "ENV", action = ArgAction::Set)]
    pub env: Option<String>,

    /// Target region (au | eu | us). Case-insensitive. Required when an env defines >1 region.
    #[arg(long, global = true, value_name = "REGION", action = ArgAction::Set)]
    pub region: Option<String>,

    /// Path to a `regions.yaml` config file. Overrides `$GOCTL_CONFIG` / XDG / `~/.config/goctl/regions.yaml`.
    #[arg(long, global = true, value_name = "PATH", action = ArgAction::Set)]
    pub config: Option<PathBuf>,

    /// Repeated `--var KEY=VALUE` template variables. Precedence: --var > $GGO_VAR_<KEY> > default.
    #[arg(long, global = true, value_name = "KEY=VALUE", action = ArgAction::Append)]
    pub var: Vec<String>,

    /// Caller-supplied bearer token. Skips OAuth and uses this JWT (validates `exp`).
    /// Alternative: set `GGO_BEARER_TOKEN`. `--token` wins if both are set.
    #[arg(long, global = true, value_name = "TOKEN", action = ArgAction::Set)]
    pub token: Option<String>,

    /// Override the configured OAuth2 `token_url` for this invocation only.
    /// Other auth fields still resolve from `regions.yaml`.
    #[arg(long, global = true, value_name = "URL", action = ArgAction::Set)]
    pub token_url: Option<String>,

    /// Resolve and render, but do NOT contact upstream or fetch an auth token.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub dry_run: bool,

    /// Required for non-GET prod requests. Not required when `--dry-run` is set.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub confirm: bool,

    /// Pretty-print object responses as JSON (default), or as a table with `--table`.
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub json: bool,

    /// Render array responses as a table when possible (falls back to JSON for heterogeneous arrays).
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub table: bool,

    /// Emit errors as single-line JSON on stderr (schema_version "1").
    #[arg(long, global = true, action = ArgAction::SetTrue)]
    pub json_errors: bool,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// List every available endpoint (machine-readable with `--json`).
    #[command(name = "list-endpoints")]
    ListEndpoints(list::ListArgs),

    /// Describe a single endpoint by service + command (JSON contract for LLM agents).
    Describe(describe::DescribeArgs),

    /// Designer endpoints.
    Designer(ServiceArgs),

    /// Captain v1 endpoints (legacy NestJS service).
    #[command(name = "captain-v1")]
    CaptainV1(ServiceArgs),

    /// Captain v2 endpoints (Hono service).
    #[command(name = "captain-v2")]
    CaptainV2(ServiceArgs),

    /// UserView endpoints.
    Userview(ServiceArgs),
}

/// Generic per-service subcommand — `<command>` is the manifest name.
#[derive(clap::Args, Debug)]
pub struct ServiceArgs {
    /// Endpoint command name (e.g. `journey-start`). See `goctl list-endpoints --service <SVC>`.
    pub command: String,
}

impl Cli {
    /// Inspect raw `std::env::args()` for duplicate safety-critical flags before clap parses.
    /// `--env`, `--region`, `--config`, `--token`, `--token-url`: reject duplicates (last-wins
    /// is dangerous for these). `--var KEY` duplicates warn on stderr.
    pub fn enforce_no_duplicate_flags<I, S>(args: I) -> CliResult<()>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut seen_env = 0u32;
        let mut seen_region = 0u32;
        let mut seen_config = 0u32;
        let mut seen_token = 0u32;
        let mut seen_token_url = 0u32;
        let mut seen_var_keys: BTreeMap<String, u32> = BTreeMap::new();

        let mut iter = args.into_iter().peekable();
        // Skip argv[0].
        iter.next();
        while let Some(a) = iter.next() {
            let s = a.as_ref();
            let (flag, _attached_value) = if let Some(eq) = s.find('=') {
                (&s[..eq], Some(&s[eq + 1..]))
            } else {
                (s, None)
            };
            match flag {
                "--env" => seen_env += 1,
                "--region" => seen_region += 1,
                "--config" => seen_config += 1,
                "--token" => seen_token += 1,
                "--token-url" => seen_token_url += 1,
                "--var" => {
                    let val_owned: Option<String> = if let Some(eq) = s.find('=') {
                        Some(s[eq + 1..].to_string())
                    } else {
                        iter.next().map(|v| v.as_ref().to_string())
                    };
                    if let Some(val) = val_owned {
                        if let Some(key_end) = val.find('=') {
                            let key = val[..key_end].to_string();
                            *seen_var_keys.entry(key).or_insert(0) += 1;
                        }
                    }
                }
                _ => {}
            }
        }
        if seen_env > 1 {
            return Err(CliError::usage(
                "--env specified more than once".to_string(),
            ));
        }
        if seen_region > 1 {
            return Err(CliError::usage(
                "--region specified more than once".to_string(),
            ));
        }
        if seen_config > 1 {
            return Err(CliError::usage(
                "--config specified more than once".to_string(),
            ));
        }
        if seen_token > 1 {
            return Err(CliError::usage(
                "--token specified more than once".to_string(),
            ));
        }
        if seen_token_url > 1 {
            return Err(CliError::usage(
                "--token-url specified more than once".to_string(),
            ));
        }
        for (k, n) in seen_var_keys {
            if n > 1 {
                eprintln!("warning: --var {k} specified {n} times; last occurrence wins");
            }
        }
        Ok(())
    }

    pub fn parse_var_pairs(raw: &[String]) -> CliResult<BTreeMap<String, String>> {
        let mut out = BTreeMap::new();
        for entry in raw {
            let (k, v) = entry.split_once('=').ok_or_else(|| {
                CliError::usage(format!("--var must be KEY=VALUE; got `{entry}`"))
            })?;
            if k.is_empty() {
                return Err(CliError::usage(format!("--var has empty key in `{entry}`")));
            }
            // Last occurrence wins (warning emitted upstream).
            out.insert(k.to_string(), v.to_string());
        }
        Ok(out)
    }
}

/// Dispatch a parsed CLI invocation. Returns a `CliError` for any failure.
pub async fn dispatch(cli: Cli) -> CliResult<()> {
    let catalog = Catalog::load_embedded()?;

    // Move the command out first so the remaining `cli` is fully owned.
    let Cli {
        env,
        region,
        config,
        var,
        token,
        token_url,
        dry_run,
        confirm,
        json: _,
        table,
        json_errors: _,
        command,
    } = cli;

    let globals = Globals {
        env,
        region,
        config,
        var,
        token,
        token_url,
        dry_run,
        confirm,
        table,
    };

    match command {
        Command::ListEndpoints(args) => list::run(args, &catalog),
        Command::Describe(args) => describe::run(args, &catalog),
        Command::Designer(svc_args) => run_service("designer", svc_args, globals, &catalog).await,
        Command::CaptainV1(svc_args) => {
            run_service("captain-v1", svc_args, globals, &catalog).await
        }
        Command::CaptainV2(svc_args) => {
            run_service("captain-v2", svc_args, globals, &catalog).await
        }
        Command::Userview(svc_args) => run_service("userview", svc_args, globals, &catalog).await,
    }
}

struct Globals {
    env: Option<String>,
    region: Option<String>,
    config: Option<PathBuf>,
    var: Vec<String>,
    token: Option<String>,
    token_url: Option<String>,
    dry_run: bool,
    confirm: bool,
    table: bool,
}

async fn run_service(
    service: &str,
    svc_args: ServiceArgs,
    cli: Globals,
    catalog: &Catalog,
) -> CliResult<()> {
    let env = cli
        .env
        .as_deref()
        .ok_or_else(|| CliError::usage("--env is required (dev | demo | prod)".to_string()))?;

    // Reject empty `--token-url ""` — silently caching an empty URL is a footgun.
    if let Some(u) = &cli.token_url {
        if u.is_empty() {
            return Err(CliError::usage("--token-url cannot be empty".to_string()));
        }
    }

    // `--token` + `--token-url` is contradictory: `--token` short-circuits the
    // OAuth flow, so the token-url override would have no effect.
    if cli.token.as_deref().is_some_and(|t| !t.is_empty())
        && cli.token_url.as_deref().is_some_and(|u| !u.is_empty())
    {
        return Err(CliError::usage(
            "--token-url has no effect when --token is supplied".to_string(),
        ));
    }

    let config = RegionsConfig::load(cli.config.as_deref())?;
    let bearer_override = bearer::resolve_override(cli.token.as_deref());

    let vars = Cli::parse_var_pairs(&cli.var)?;

    let inputs = ExecInputs {
        service,
        command: &svc_args.command,
        env,
        region: cli.region.as_deref(),
        vars,
        bearer_override,
        token_url_override: cli.token_url.clone(),
        dry_run: cli.dry_run,
        confirm: cli.confirm,
        want_table: cli.table,
    };

    crate::exec::run(catalog, &config, inputs).await
}
