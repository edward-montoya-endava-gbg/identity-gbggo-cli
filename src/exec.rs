//! End-to-end orchestrator: resolve → render → (dry-run short-circuit) → auth → HTTP → output.

use crate::auth::{self, AuthSource};
use crate::config::RegionsConfig;
use crate::endpoints::{Catalog, EndpointBody, EndpointView};
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
    /// Versioned-service version override from `--api-version`. Ignored for
    /// flat (non-versioned) services.
    pub api_version: Option<&'a str>,
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

    // Resolve target up front so we can consult `default_version` from
    // regions.yaml during version resolution.
    let mut target = config.resolve(inputs.service, inputs.env, inputs.region)?;

    // Resolve version (versioned endpoints only).
    let resolved_version: Option<String> = match &endpoint.body {
        EndpointBody::Flat(_) => None,
        EndpointBody::Versioned(vb) => {
            let chosen = resolve_version(
                inputs.service,
                &endpoint.name,
                &vb.supported_versions,
                inputs.api_version,
                target.default_version.as_deref(),
            )?;
            Some(chosen)
        }
    };

    // Build the per-version view used by render/auth/HTTP.
    let view = endpoint
        .resolve_version(resolved_version.as_deref())
        .ok_or_else(|| {
            CliError::config(format!(
                "internal: failed to materialize endpoint view for {}/{}",
                inputs.service, endpoint.name
            ))
        })?;

    // Disabled-endpoint gate. Runs BEFORE auth / var resolution / --dry-run.
    if view.disabled {
        let reason = view.disabled_reason.unwrap_or("disabled");
        let svc_dir = inputs.service.replace('-', "_");
        let hint = match view.version {
            Some(v) => format!(
                "edit src/endpoints/{}/{}.yaml and remove `disabled: true` under `versions.{}`, then run `cargo install --path . --force`",
                svc_dir, endpoint.name, v
            ),
            None => format!(
                "edit src/endpoints/{}/{}.yaml and remove `disabled: true`, then run `cargo install --path . --force`",
                svc_dir, endpoint.name
            ),
        };
        return Err(CliError::usage(format!(
            "endpoint `{}/{}` is disabled: {}. To enable, {}.",
            inputs.service, endpoint.name, reason, hint
        )));
    }

    // Deprecation warning — emit once, do not fail.
    if let Some(reason) = view.deprecated {
        if let Some(v) = view.version {
            tracing::warn!(
                "{}/{} version {} is deprecated: {}",
                inputs.service,
                endpoint.name,
                v,
                reason
            );
        }
    }

    // Pick the right base_url for the resolved version.
    let base_url = target
        .urls
        .pick(resolved_version.as_deref())
        .ok_or_else(|| match resolved_version.as_deref() {
            Some(v) => CliError::config(format!(
                "regions.yaml `{}.{}.regions.{}.base_urls` has no entry for version `{}`",
                inputs.service, inputs.env, target.region, v
            )),
            None => CliError::config(format!(
                "regions.yaml `{}.{}.regions.{}` has no base_url",
                inputs.service, inputs.env, target.region
            )),
        })?
        .to_string();

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
    let source = auth::pick_source_with_auth(&endpoint.auth, &target, bearer_override)?;

    // Required-var enforcement and JSON validation happens here, BEFORE network/HTTP.
    let vars = render::resolve_vars(&view, &inputs.vars)?;
    let body = render::render_body(&view, &vars)?;

    // Path substitution: any `{name}` literal in the path is replaced with the
    // string-typed var value (URL-path-encoded).
    let path = render::substitute_path(view.path, &vars)?;
    let url = format!("{}{}", base_url, path);
    let method = view.method.to_ascii_uppercase();

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
    )
    .await
}

/// Resolve which version to use for a versioned endpoint.
///
/// Order:
/// 1. explicit `--api-version` flag (always wins when supplied);
/// 2. if the endpoint has exactly one supported version → that version (no
///    further consultation — avoids the footgun where a global
///    `default_version: v2` collides with a v1-only endpoint);
/// 3. regions.yaml `default_version` (only when the endpoint genuinely
///    supports multiple versions and no flag was supplied);
/// 4. error listing the supported versions.
fn resolve_version(
    service: &str,
    name: &str,
    supported: &[String],
    flag: Option<&str>,
    default_version: Option<&str>,
) -> CliResult<String> {
    let chosen = if let Some(v) = flag {
        v.to_string()
    } else if supported.len() == 1 {
        supported[0].clone()
    } else if let Some(v) = default_version {
        v.to_string()
    } else {
        return Err(CliError::usage(format!(
            "endpoint `{service}/{name}` exists in versions [{}]; pass --api-version or set default_version in regions.yaml",
            supported.join(", ")
        )));
    };

    if !supported.contains(&chosen) {
        return Err(CliError::usage(format!(
            "endpoint `{service}/{name}` does not support version `{chosen}`; available: [{}]",
            supported.join(", ")
        )));
    }

    Ok(chosen)
}

/// Idempotent HTTP methods are safe to retry on transient failure.
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
) -> CliResult<()> {
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

// Unused helper retained for tests that look up an EndpointView for a service+command+version.
#[allow(dead_code)]
pub fn lookup_view<'a>(
    catalog: &'a Catalog,
    service: &str,
    command: &str,
    version: Option<&str>,
) -> Option<EndpointView<'a>> {
    catalog.get(service, command)?.resolve_version(version)
}
