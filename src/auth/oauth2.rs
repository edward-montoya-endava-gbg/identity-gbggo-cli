//! OAuth2 client: `client_credentials` and `password` grants only.
//!
//! Hardening:
//! - Token POST disables redirect-following (so secrets cannot be re-POSTed cross-host).
//! - Network errors (DNS, TCP reset, TLS, timeout) → `CliError::Network` (one-retry path).
//! - `expires_in <= 0` → `CliError::Auth`.

use crate::config::{GrantType, OAuth2Config};
use crate::error::{CliError, CliResult};
use serde::Deserialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub struct FetchedToken {
    pub access_token: String,
    /// Absolute unix seconds when the token expires (computed as `now + expires_in`).
    pub exp: i64,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    #[serde(default)]
    expires_in: Option<i64>,
}

/// Build a `reqwest::Client` dedicated to the OAuth token POST. This client does
/// NOT follow redirects.
pub fn token_client() -> reqwest::Client {
    reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .expect("rustls reqwest client should always build")
}

pub async fn fetch_token(_outer: &reqwest::Client, cfg: &OAuth2Config) -> CliResult<FetchedToken> {
    let client = token_client();

    let client_id = read_client_id(cfg)?;

    let secret_env = &cfg.client_secret_env;
    let client_secret = std::env::var(secret_env).map_err(|_| {
        CliError::auth(format!(
            "missing env var `{secret_env}` for OAuth2 client_secret"
        ))
    })?;
    if client_secret.is_empty() {
        return Err(CliError::auth(format!(
            "env var `{secret_env}` is set but empty"
        )));
    }

    let mut params: Vec<(&str, String)> = Vec::new();
    match cfg.grant_type {
        GrantType::ClientCredentials => {
            params.push(("grant_type", "client_credentials".to_string()));
            params.push(("client_id", client_id.clone()));
            params.push(("client_secret", client_secret));
            if let Some(scope) = &cfg.scope {
                params.push(("scope", scope.clone()));
            }
        }
        GrantType::Password => {
            let user_env = cfg.username_env.as_ref().ok_or_else(|| {
                CliError::config("password grant config has no username_env".to_string())
            })?;
            let pass_env = cfg.password_env.as_ref().ok_or_else(|| {
                CliError::config("password grant config has no password_env".to_string())
            })?;
            let user_missing = std::env::var(user_env).is_err();
            let pass_missing = std::env::var(pass_env).is_err();
            if user_missing || pass_missing {
                let mut missing: Vec<&str> = Vec::new();
                if user_missing {
                    missing.push(user_env);
                }
                if pass_missing {
                    missing.push(pass_env);
                }
                return Err(CliError::auth(format!(
                    "password grant requires env vars: {} (not set)",
                    missing.join(", ")
                )));
            }
            params.push(("grant_type", "password".to_string()));
            params.push(("client_id", client_id));
            params.push(("client_secret", client_secret));
            params.push(("username", std::env::var(user_env).unwrap()));
            params.push(("password", std::env::var(pass_env).unwrap()));
            if let Some(scope) = &cfg.scope {
                params.push(("scope", scope.clone()));
            }
        }
    }

    // One-retry on network errors only (DNS/TCP reset/TLS/timeout). Mirrors the
    // data-plane GET retry behavior. We do NOT retry on auth-class errors (401/403,
    // non-JSON body) or on 5xx — token URL semantics differ from data-plane.
    let mut attempt: u8 = 0;
    let resp = loop {
        attempt += 1;
        let req = client
            .post(&cfg.token_url)
            .header("Accept", "application/json")
            .form(&params);
        match req.send().await {
            Ok(r) => break r,
            Err(e) => {
                let is_net = e.is_timeout() || e.is_connect() || e.is_request();
                if is_net && attempt == 1 {
                    tokio::time::sleep(Duration::from_millis(500)).await;
                    continue;
                }
                if is_net {
                    return Err(CliError::network(format!(
                        "token URL {} unreachable: {e}",
                        cfg.token_url
                    )));
                }
                return Err(CliError::auth(format!(
                    "token URL {} POST failed: {e}",
                    cfg.token_url
                )));
            }
        }
    };

    // Detect redirect attempt — with Policy::none the client surfaces a 3xx instead of following.
    let status = resp.status();
    if status.is_redirection() {
        return Err(CliError::auth(format!(
            "token URL {} responded {} (redirect); redirect-following is disabled to protect secrets",
            cfg.token_url, status
        )));
    }
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        let snippet = truncate_chars(&body, 200);
        return Err(CliError::auth(format!(
            "token URL {} responded {} ({})",
            cfg.token_url, status, snippet
        )));
    }

    let body_text = resp.text().await.map_err(|e| {
        CliError::auth(format!(
            "token URL {} response body read failed: {e}",
            cfg.token_url
        ))
    })?;
    let parsed: TokenResponse = serde_json::from_str(&body_text).map_err(|e| {
        CliError::auth(format!(
            "token URL {} returned non-JSON body: {e}; first 200 chars: {}",
            cfg.token_url,
            truncate_chars(&body_text, 200)
        ))
    })?;

    let expires_in = parsed.expires_in.unwrap_or(3600);
    if expires_in <= 0 {
        return Err(CliError::auth(format!(
            "token URL {} returned expires_in={}; refusing to cache",
            cfg.token_url, expires_in
        )));
    }

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);

    Ok(FetchedToken {
        access_token: parsed.access_token,
        exp: now + expires_in,
    })
}

/// Resolve the OAuth2 `client_id` from the env var named in `cfg.client_id_env`.
/// Missing or empty env var → `CliError::Auth` (exit 3).
fn read_client_id(cfg: &OAuth2Config) -> CliResult<String> {
    let client_id = std::env::var(&cfg.client_id_env).map_err(|_| {
        CliError::auth(format!(
            "missing env var {} for OAuth2 client_id",
            cfg.client_id_env
        ))
    })?;
    if client_id.is_empty() {
        return Err(CliError::auth(format!(
            "env var {} is set but empty",
            cfg.client_id_env
        )));
    }
    Ok(client_id)
}

fn truncate_chars(s: &str, n: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= n {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}
