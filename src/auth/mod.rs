//! Auth orchestration. `pick_source` reads endpoint-level `auth` FIRST so that
//! `auth: none` endpoints bypass both the prod bearer-only gate and any OAuth flow.

pub mod bearer;
pub mod oauth2;

use crate::config::{OAuth2Config, ResolvedTarget};
use crate::endpoints::{Auth as EndpointAuth, EndpointDef};
use crate::error::{CliError, CliResult};

#[derive(Debug, Clone)]
pub enum AuthSource {
    /// Endpoint declared `auth: none` — no `Authorization` header at all.
    None,
    /// A caller-supplied bearer (decoded `exp` already validated).
    Bearer(String),
    /// OAuth2 flow per the configured grant.
    Oauth2 {
        config: OAuth2Config,
        env: String,
        service: String,
        region: String,
    },
}

/// Pick the auth source given an endpoint manifest + target + caller bearer override.
pub fn pick_source(
    endpoint: &EndpointDef,
    target: &ResolvedTarget,
    bearer_override: Option<&str>,
) -> CliResult<AuthSource> {
    pick_source_with_auth(&endpoint.auth, target, bearer_override)
}

/// Pick the auth source given a service-level `Auth` (Bearer/None) + target +
/// caller bearer override. Used by the exec layer where the auth lives on the
/// EndpointDef itself (not the per-version block).
///
/// Precedence:
/// 1. `auth == None` → `AuthSource::None` (bypasses everything else).
/// 2. Caller-supplied bearer (`--token` or `GGO_BEARER_TOKEN`) → validate + use.
/// 3. `target.auth: None` (config says bearer-only) → error unless step 2 caught it.
/// 4. Configured OAuth2 → run grant.
pub fn pick_source_with_auth(
    endpoint_auth: &EndpointAuth,
    target: &ResolvedTarget,
    bearer_override: Option<&str>,
) -> CliResult<AuthSource> {
    if matches!(endpoint_auth, EndpointAuth::None) {
        // Endpoint is `auth: none`, so no Authorization header will be sent.
        if let Some(token) = bearer_override {
            if let Ok(exp) = bearer::extract_exp(token) {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                if exp <= now {
                    return Err(CliError::auth(format!(
                        "supplied --token is expired (exp={exp}, now={now}); endpoint is auth: none so the token would not have been sent"
                    )));
                }
            }
        }
        return Ok(AuthSource::None);
    }

    if let Some(token) = bearer_override {
        bearer::validate_jwt_exp(token)?;
        return Ok(AuthSource::Bearer(token.to_string()));
    }

    match &target.auth {
        None => Err(CliError::usage(format!(
            "{} in {} is bearer-only; supply --token or GGO_BEARER_TOKEN",
            target.service, target.env
        ))),
        Some(cfg) => Ok(AuthSource::Oauth2 {
            config: cfg.clone(),
            env: target.env.clone(),
            service: target.service.clone(),
            region: target.region.clone(),
        }),
    }
}

/// Resolve a concrete `Authorization` header value (or `None` for `auth: none` endpoints).
pub async fn header_for(
    source: &AuthSource,
    client: &reqwest::Client,
) -> CliResult<Option<String>> {
    match source {
        AuthSource::None => Ok(None),
        AuthSource::Bearer(t) => Ok(Some(format!("Bearer {t}"))),
        AuthSource::Oauth2 {
            config,
            env: _,
            service: _,
            region: _,
        } => {
            let token = oauth2::fetch_token(client, config).await?;
            Ok(Some(format!("Bearer {}", token.access_token)))
        }
    }
}
