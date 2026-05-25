//! Reject any literal `client_secret` / `password` / `token` field under `auth`.

use goctl::config::RegionsConfig;
use std::io::Write;

fn write_tmp(contents: &str) -> tempfile::NamedTempFile {
    let mut tf = tempfile::NamedTempFile::new().unwrap();
    tf.write_all(contents.as_bytes()).unwrap();
    tf.flush().unwrap();
    tf
}

#[test]
fn rejects_literal_client_secret() {
    let yaml = r#"
designer:
  envs:
    dev:
      regions:
        eu: { base_url: http://example }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id_env: GGO_TEST_REJECTS_LITERAL_CLIENT_SECRET
        client_secret_env: GGO_TEST_REJECTS_LITERAL_CLIENT_SECRET_SECRET
        client_secret: SHOULD_FAIL
"#;
    let tf = write_tmp(yaml);
    let err = RegionsConfig::load_from(tf.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown field"), "got: {msg}");
    assert!(msg.contains("client_secret"), "got: {msg}");
}

#[test]
fn rejects_literal_password() {
    let yaml = r#"
designer:
  envs:
    dev:
      regions:
        eu: { base_url: http://example }
      auth:
        grant_type: password
        token_url: http://example/token
        client_id_env: GGO_TEST_REJECTS_LITERAL_PASSWORD
        client_secret_env: GGO_TEST_REJECTS_LITERAL_PASSWORD_SECRET
        username_env: U
        password_env: P
        password: SHOULD_FAIL
"#;
    let tf = write_tmp(yaml);
    let err = RegionsConfig::load_from(tf.path()).unwrap_err();
    assert!(err.to_string().contains("unknown field"), "got: {}", err);
}

#[test]
fn rejects_literal_token() {
    let yaml = r#"
designer:
  envs:
    dev:
      regions:
        eu: { base_url: http://example, token: SHOULD_FAIL }
      auth: null
"#;
    let tf = write_tmp(yaml);
    let err = RegionsConfig::load_from(tf.path()).unwrap_err();
    assert!(err.to_string().contains("unknown field"), "got: {}", err);
}

#[test]
fn rejects_literal_client_id() {
    // After the schema rename, a literal `client_id` is no longer a recognized
    // field — operators must reference an env var via `client_id_env`.
    let yaml = r#"
designer:
  envs:
    dev:
      regions:
        eu: { base_url: http://example }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id: SHOULD_FAIL
        client_secret_env: GGO_DESIGNER_DEV_CLIENT_SECRET
"#;
    let tf = write_tmp(yaml);
    let err = RegionsConfig::load_from(tf.path()).unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("unknown field"), "got: {msg}");
    assert!(msg.contains("client_id"), "got: {msg}");
}

#[test]
fn rejects_unknown_service_key() {
    let yaml = r#"
acme-bogus:
  envs:
    dev:
      regions:
        eu: { base_url: http://example }
      auth: null
"#;
    let tf = write_tmp(yaml);
    let err = RegionsConfig::load_from(tf.path()).unwrap_err();
    assert!(err.to_string().contains("unknown service"), "got: {}", err);
}

#[test]
fn accepts_only_env_references() {
    let yaml = r#"
designer:
  envs:
    dev:
      regions:
        eu: { base_url: http://example }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id_env: GGO_TEST_ACCEPTS_ONLY_ENV_REFERENCES
        client_secret_env: GGO_DESIGNER_DEV_CLIENT_SECRET
"#;
    let tf = write_tmp(yaml);
    let cfg = RegionsConfig::load_from(tf.path()).unwrap();
    assert!(cfg.services.contains_key("designer"));
}
