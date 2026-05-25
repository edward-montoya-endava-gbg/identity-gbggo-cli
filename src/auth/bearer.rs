//! Caller-supplied bearer: parse JWT `exp` without signature verification.
//!
//! - Accepts `exp` as int, string, or float (custom deserializer).
//! - 30s skew tolerance: rejection if `exp - 30 <= now`.
//! - JWE detection (5 segments) → explicit error.
//! - Distinguishes "payload is not JSON" vs "payload lacks exp".
//! - Empty `--token ""` filtered same as empty env var.

use crate::error::{CliError, CliResult};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use serde::{Deserialize, Deserializer};
use std::time::{SystemTime, UNIX_EPOCH};

const SKEW_SECS: i64 = 30;

#[derive(Deserialize)]
struct JwtClaims {
    #[serde(deserialize_with = "deser_exp")]
    exp: i64,
}

/// Accept `exp` as int, string, or float.
fn deser_exp<'de, D>(d: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    use serde::de::Error;
    let v = serde_json::Value::deserialize(d)?;
    match v {
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i)
            } else if let Some(f) = n.as_f64() {
                Ok(f as i64)
            } else {
                Err(Error::custom("exp must be a number representable as i64"))
            }
        }
        serde_json::Value::String(s) => s
            .parse::<i64>()
            .or_else(|_| s.parse::<f64>().map(|f| f as i64))
            .map_err(|_| Error::custom("exp string is not numeric")),
        other => Err(Error::custom(format!(
            "exp must be number or string, got {}",
            json_type(&other)
        ))),
    }
}

fn json_type(v: &serde_json::Value) -> &'static str {
    match v {
        serde_json::Value::Null => "null",
        serde_json::Value::Bool(_) => "bool",
        serde_json::Value::Number(_) => "number",
        serde_json::Value::String(_) => "string",
        serde_json::Value::Array(_) => "array",
        serde_json::Value::Object(_) => "object",
    }
}

/// Decode `exp` from a JWT payload without verifying its signature.
/// Returns the `exp` value in seconds since epoch.
pub fn extract_exp(token: &str) -> CliResult<i64> {
    let segs: Vec<&str> = token.split('.').collect();
    match segs.len() {
        3 => {}
        5 => {
            return Err(CliError::auth(
                "JWE encrypted tokens are not supported (5 segments); supply a JWS bearer"
                    .to_string(),
            ));
        }
        _ => {
            return Err(CliError::usage(
                "--token does not parse as a JWT (cannot read exp): expected 3 segments"
                    .to_string(),
            ));
        }
    }
    let payload_b64 = segs[1];
    let payload = URL_SAFE_NO_PAD
        .decode(payload_b64)
        .map_err(|e| CliError::usage(format!("--token payload is not valid base64url: {e}")))?;

    // Distinguish "not JSON" from "lacks exp".
    let value: serde_json::Value = serde_json::from_slice(&payload)
        .map_err(|e| CliError::usage(format!("--token payload is not JSON: {e}")))?;
    if value.get("exp").is_none() {
        return Err(CliError::usage(
            "--token payload lacks an `exp` claim".to_string(),
        ));
    }

    let claims: JwtClaims = serde_json::from_value(value)
        .map_err(|e| CliError::usage(format!("--token `exp` claim is malformed: {e}")))?;
    Ok(claims.exp)
}

/// Validate that the bearer has a future `exp` (with 30s skew tolerance).
pub fn validate_jwt_exp(token: &str) -> CliResult<()> {
    let exp = extract_exp(token)?;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if exp - SKEW_SECS <= now {
        return Err(CliError::auth(format!(
            "--token is expired (exp={exp}, now={now}, skew={SKEW_SECS}s); supply a fresh bearer"
        )));
    }
    Ok(())
}

/// Resolve a bearer override from `--token` and `GGO_BEARER_TOKEN`. Empty strings
/// are filtered out the same way for both sources.
pub fn resolve_override(cli_token: Option<&str>) -> Option<String> {
    if let Some(t) = cli_token {
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    if let Ok(t) = std::env::var("GGO_BEARER_TOKEN") {
        if !t.is_empty() {
            return Some(t);
        }
    }
    None
}
