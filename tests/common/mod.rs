//! Shared test helpers.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use std::time::{SystemTime, UNIX_EPOCH};

/// Build an unsigned (sig segment empty) JWT with the given `exp` (seconds since epoch).
pub fn make_jwt(exp_offset_secs: i64) -> String {
    let header = serde_json::json!({ "alg": "none", "typ": "JWT" });
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let claims = serde_json::json!({ "exp": now + exp_offset_secs, "sub": "test" });
    let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let c = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    format!("{h}.{c}.")
}

/// Build a JWT whose `exp` claim is the given JSON value (used to test int / string / float).
#[allow(dead_code)]
pub fn make_jwt_with_raw_exp(raw_exp_json: serde_json::Value) -> String {
    let header = serde_json::json!({ "alg": "none", "typ": "JWT" });
    let claims = serde_json::json!({ "exp": raw_exp_json, "sub": "test" });
    let h = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&header).unwrap());
    let c = URL_SAFE_NO_PAD.encode(serde_json::to_vec(&claims).unwrap());
    format!("{h}.{c}.")
}
