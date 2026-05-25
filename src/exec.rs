//! End-to-end orchestrator: resolve → render → (dry-run short-circuit) → auth → HTTP → output.

use crate::auth::{self, AuthSource};
use crate::config::RegionsConfig;
use crate::endpoints::{Catalog, EndpointDef};
use crate::error::{CliError, CliResult, ExitKind};
use crate::output;
use crate::render;
use std::collections::BTreeMap;
use std::time::Duration;

/// Inputs that control an execution (everything resolved from CLI flags).
pub struct ExecInputs<'a> {
    pub service: &'a str,
    pub command: &'a str,
    pub env: &'a str,
    pub region: Option<&'a str>,
    pub vars: BTreeMap<String, String>,
    pub bearer_override: Option<String>,
    pub token_url_override: Option<String>,
    pub dry_run: bool,
    pub confirm: bool,
    pub want_table: bool,
}

pub async fn run(
    catalog: &Catalog,
    config: &RegionsConfig,
    inputs: ExecInputs<'_>,
) -> CliResult<()> {
    let endpoint = catalog.get(inputs.service, inputs.command).ok_or_else(|| {
        CliError::usage(format!(
            "command `{}` not found for service `{}`",
            inputs.command, inputs.service
        ))
    })?;

    let mut target = config.resolve(inputs.service, inputs.env, inputs.region)?;

    // Apply --token-url override (only meaningful when an OAuth source will be used).
    if let Some(url) = &inputs.token_url_override {
        match &mut target.auth {
            Some(cfg) => {
                cfg.token_url = url.clone();
            }
            None => {
                // Bearer-only tuple — overriding the token URL has no effect. Reject loudly.
                // Skip if endpoint is auth: none — overriding is harmless then but still useless.
                if !matches!(endpoint.auth, crate::endpoints::Auth::None) {
                    return Err(CliError::usage(
                        "--token-url has no effect: target is bearer-only".to_string(),
                    ));
                }
            }
        }
    }

    // Pick auth source FIRST so the bearer-only prod gate fires before any var/env
    // read (per the spec contract).
    let bearer_override = inputs.bearer_override.as_deref();
    let source = auth::pick_source(endpoint, &target, bearer_override)?;

    // Required-var enforcement and JSON validation happens here, BEFORE network/HTTP.
    let vars = render::resolve_vars(endpoint, &inputs.vars)?;
    let body = render::render_body(endpoint, &vars)?;

    // Path substitution: any `{name}` literal in the path is replaced with the
    // string-typed var value (URL-path-encoded).
    let path = render::substitute_path(&endpoint.path, &vars)?;
    let url = format!("{}{}", target.base_url, path);
    let method = endpoint.method.to_ascii_uppercase();

    // Prod write gate: any non-GET against prod requires --confirm UNLESS --dry-run is set.
    let is_write = !matches!(method.as_str(), "GET" | "HEAD" | "OPTIONS");
    if inputs.env == "prod" && is_write && !inputs.confirm && !inputs.dry_run {
        return Err(CliError::usage(format!(
            "{} {} is a write against prod; pass --confirm (or --dry-run to preview)",
            method, url
        )));
    }

    if inputs.dry_run {
        let has_auth = !matches!(source, AuthSource::None);
        output::print_dry_run(&method, &url, has_auth, body.as_deref());
        return Ok(());
    }

    // Build the request client.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .map_err(|e| CliError::config(format!("client build error: {e}")))?;

    let auth_header = auth::header_for(&source, &client).await?;

    do_request(
        &client,
        &method,
        &url,
        auth_header.as_deref(),
        body.as_deref(),
        inputs.want_table,
        endpoint,
    )
    .await
}

/// Idempotent HTTP methods are safe to retry on transient failure.
/// Per spec Loopback #2: retry only fires for GET/HEAD/OPTIONS. All other
/// methods (POST/PUT/PATCH/DELETE) fail-fast to prevent silent double-execute.
fn is_idempotent(method: &str) -> bool {
    method.eq_ignore_ascii_case("GET")
        || method.eq_ignore_ascii_case("HEAD")
        || method.eq_ignore_ascii_case("OPTIONS")
}

async fn do_request(
    client: &reqwest::Client,
    method: &str,
    url: &str,
    auth_header: Option<&str>,
    body: Option<&str>,
    want_table: bool,
    _endpoint: &EndpointDef,
) -> CliResult<()> {
    // One-retry path is gated on idempotent methods only. For non-idempotent
    // methods, the first failure (5xx or network) is the final answer.
    let retry_allowed = is_idempotent(method);
    let mut attempt: u8 = 0;
    loop {
        attempt += 1;
        let mut req = client.request(
            method
                .parse()
                .map_err(|e| CliError::config(format!("invalid HTTP method `{method}`: {e}")))?,
            url,
        );
        if let Some(a) = auth_header {
            req = req.header("Authorization", a);
        }
        if let Some(b) = body {
            req = req
                .header("Content-Type", "application/json")
                .body(b.to_string());
        }

        match req.send().await {
            Ok(resp) => {
                let status = resp.status();
                let request_id = resp
                    .headers()
                    .get("X-Request-ID")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string());
                let bytes = resp.bytes().await.unwrap_or_default();
                let text = output::body_lossy(&bytes);

                if status.is_success() {
                    return output::print_response(&text, want_table);
                }
                if status.is_server_error() && attempt == 1 && retry_allowed {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                let msg = output::upstream_error_message(
                    method,
                    url,
                    status.as_u16(),
                    request_id.as_deref(),
                    &text,
                );
                if status.is_client_error() {
                    if status.as_u16() == 401 || status.as_u16() == 403 {
                        return Err(CliError::new(ExitKind::Auth, msg));
                    }
                    return Err(CliError::upstream_client(msg));
                }
                return Err(CliError::upstream_server(msg));
            }
            Err(e) => {
                let is_net = e.is_timeout() || e.is_connect() || e.is_request() || e.is_body();
                if is_net && attempt == 1 && retry_allowed {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                let kind = if is_net {
                    ExitKind::Network
                } else {
                    ExitKind::UpstreamServer
                };
                return Err(CliError::new(kind, format!("{method} {url} failed: {e}")));
            }
        }
    }
}
