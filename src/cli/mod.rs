//! CLI surface — root parser, global flags, dispatch.

pub mod describe;
pub mod describe_fixture;
pub mod list;
pub mod list_fixtures;

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
    about = "GBG GO API CLI — Designer, Captain (v1/v2/...), UserView.",
    long_about = "GBG GO API CLI — Designer, Captain (versioned), UserView.\n\n\
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
    /// VALUE may use the `@<path>` prefix to load the value from a file
    /// (YAML/JSON auto-detected by extension).
    #[arg(long, global = true, value_name = "KEY=VALUE", action = ArgAction::Append)]
    pub var: Vec<String>,

    /// Compose Captain `context` payloads by referencing built-in test-data
    /// fixtures. Repeatable. Each occurrence merges
    /// `context.<CATEGORY> = <fixture-json>`. Pure convenience sugar — raw
    /// JSON via `--var context='<json>'` remains the canonical path and
    /// behaves identically when no `--include` is supplied.
    ///
    /// CATEGORY is the fixture family (e.g. `identity`, `documents`,
    /// `biometrics`). NAME is a built-in fixture name (see `goctl
    /// list-fixtures`) or a filesystem path containing `/` or `.`.
    #[arg(
        long = "include",
        global = true,
        value_names = ["CATEGORY", "NAME"],
        num_args = 2,
        action = ArgAction::Append,
    )]
    pub include: Vec<String>,

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

    /// Pick a specific API version for a versioned service (e.g. `v1`, `v2`).
    /// Required when an endpoint supports multiple versions and no
    /// `default_version` is configured for the env in regions.yaml. Ignored
    /// for non-versioned services.
    #[arg(long = "api-version", global = true, value_name = "V", action = ArgAction::Set)]
    pub api_version: Option<String>,

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

    /// List every built-in test-data fixture (`--include` candidates).
    #[command(name = "list-fixtures")]
    ListFixtures(list_fixtures::ListFixturesArgs),

    /// Print a built-in fixture's resolved JSON (post `@file:` inlining).
    #[command(name = "describe-fixture")]
    DescribeFixture(describe_fixture::DescribeFixtureArgs),

    /// Designer endpoints.
    Designer(ServiceArgs),

    /// Captain endpoints. Versioned via the global `--api-version` flag.
    Captain(ServiceArgs),

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
    /// `--env`, `--region`, `--config`, `--token`, `--token-url`, `--api-version`: reject
    /// duplicates (last-wins is dangerous for these). `--var KEY` duplicates warn on stderr.
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
        let mut seen_api_version = 0u32;
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
                "--api-version" => seen_api_version += 1,
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
        if seen_api_version > 1 {
            return Err(CliError::usage(
                "--api-version specified more than once".to_string(),
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
            let value = expand_at_file_value(k, v)?;
            // Last occurrence wins (warning emitted upstream).
            out.insert(k.to_string(), value);
        }
        Ok(out)
    }
}

/// Expand a `--var KEY=VALUE` value following the curl `@<path>` convention.
///
/// - `VALUE` starting with `\@` → strip the backslash, return the literal text
///   (escape for a real `@` first byte).
/// - `VALUE` starting with `@` → read the file at the remaining path. When the
///   file extension is `.yaml` / `.yml`, parse it as YAML and re-serialize as
///   compact JSON so `kind: json` vars validate correctly downstream and
///   `kind: string` vars receive a canonical JSON snapshot. Other extensions
///   (`.json`, `.txt`, no extension) return the raw UTF-8 file contents.
/// - Anything else → returned verbatim.
fn expand_at_file_value(key: &str, value: &str) -> CliResult<String> {
    if let Some(rest) = value.strip_prefix("\\@") {
        return Ok(format!("@{rest}"));
    }
    let Some(path_str) = value.strip_prefix('@') else {
        return Ok(value.to_string());
    };
    let path = std::path::Path::new(path_str);
    let raw = std::fs::read_to_string(path).map_err(|e| {
        CliError::usage(format!(
            "--var {key}=@{path_str}: file not found / not readable ({e})"
        ))
    })?;
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(str::to_ascii_lowercase);
    match ext.as_deref() {
        Some("yaml") | Some("yml") => {
            let v: serde_json::Value = serde_yaml::from_str(&raw).map_err(|e| {
                CliError::usage(format!("--var {key}=@{path_str}: YAML parse error: {e}"))
            })?;
            serde_json::to_string(&v).map_err(|e| {
                CliError::config(format!(
                    "--var {key}=@{path_str}: failed to serialize YAML → JSON: {e}"
                ))
            })
        }
        _ => Ok(raw),
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
        include,
        token,
        token_url,
        dry_run,
        confirm,
        json: _,
        table,
        json_errors: _,
        api_version,
        command,
    } = cli;

    let globals = Globals {
        env,
        region,
        config,
        var,
        include,
        token,
        token_url,
        dry_run,
        confirm,
        table,
        api_version,
    };

    match command {
        Command::ListEndpoints(args) => list::run(args, &catalog),
        Command::Describe(args) => describe::run(args, &catalog),
        Command::ListFixtures(args) => list_fixtures::run(args),
        Command::DescribeFixture(args) => describe_fixture::run(args),
        Command::Designer(svc_args) => run_service("designer", svc_args, globals, &catalog).await,
        Command::Captain(svc_args) => run_service("captain", svc_args, globals, &catalog).await,
        Command::Userview(svc_args) => run_service("userview", svc_args, globals, &catalog).await,
    }
}

struct Globals {
    env: Option<String>,
    region: Option<String>,
    config: Option<PathBuf>,
    var: Vec<String>,
    include: Vec<String>,
    token: Option<String>,
    token_url: Option<String>,
    dry_run: bool,
    confirm: bool,
    table: bool,
    api_version: Option<String>,
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

    let mut vars = Cli::parse_var_pairs(&cli.var)?;
    apply_includes(&cli.include, &mut vars)?;

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
        api_version: cli.api_version.as_deref(),
    };

    crate::exec::run(catalog, &config, inputs).await
}

/// Apply `--include CATEGORY NAME` overlays onto the `context` var. Each pair
/// is resolved against the fixture catalog (built-in first, filesystem
/// fallback when NAME contains `/` or `.`), then merged into a JSON object
/// keyed by category. The merged object becomes the resolved value of the
/// `context` var, overriding any earlier `--var context=…` value in the
/// resolved-vars map (which seeds the base for `context_base`).
///
/// Silently a no-op when no `--include` flags are supplied — raw JSON via
/// `--var context='{...}'` remains the canonical path.
fn apply_includes(include_pairs: &[String], vars: &mut BTreeMap<String, String>) -> CliResult<()> {
    if include_pairs.is_empty() {
        return Ok(());
    }
    if include_pairs.len() % 2 != 0 {
        return Err(CliError::usage(
            "--include requires two arguments: CATEGORY NAME".to_string(),
        ));
    }

    // Seed from any user-supplied `--var context=...` (already file-loaded by
    // parse_var_pairs / RawValue::load_var). Honor JSON-string values; otherwise
    // start from an empty object.
    let mut base: serde_json::Map<String, serde_json::Value> = match vars.get("context") {
        Some(s) if !s.is_empty() => {
            let v: serde_json::Value = serde_json::from_str(s).map_err(|e| {
                CliError::usage(format!(
                    "--var context=… must be valid JSON (combined with --include): {e}"
                ))
            })?;
            match v {
                serde_json::Value::Object(m) => m,
                other => {
                    return Err(CliError::usage(format!(
                        "--var context=… must be a JSON object when combined with --include (got {other})"
                    )));
                }
            }
        }
        _ => serde_json::Map::new(),
    };

    // Captain endpoints nest identity/documents/biometrics under
    // `context.subject.<category>`. Includes overlay there, preserving any
    // other fields the caller already put at `context` (e.g. `config`,
    // `consent`, etc.) or at `context.subject` (e.g. `uid`, `sessions`).
    let mut subject: serde_json::Map<String, serde_json::Value> = match base.remove("subject") {
        Some(serde_json::Value::Object(m)) => m,
        Some(other) => {
            return Err(CliError::usage(format!(
                "--var context=… already has `subject` set to a non-object value ({other}); cannot overlay --include fixtures into it"
            )));
        }
        None => serde_json::Map::new(),
    };

    let mut iter = include_pairs.iter();
    while let (Some(category), Some(name)) = (iter.next(), iter.next()) {
        let fixture = crate::fixtures::resolve(category, name)?;
        subject.insert(category.clone(), fixture);
    }

    base.insert(
        "subject".to_string(),
        serde_json::Value::Object(subject),
    );

    let merged = serde_json::Value::Object(base);
    let serialized = serde_json::to_string(&merged)
        .map_err(|e| CliError::config(format!("failed to serialize --include context: {e}")))?;
    vars.insert("context".to_string(), serialized);
    Ok(())
}
