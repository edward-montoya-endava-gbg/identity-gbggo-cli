//! OAuth2 + bearer auth tests via wiremock.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;
use wiremock::matchers::{body_string_contains, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

fn write_config(yaml: &str) -> tempfile::NamedTempFile {
    let mut tf = tempfile::NamedTempFile::new().unwrap();
    tf.write_all(yaml.as_bytes()).unwrap();
    tf.flush().unwrap();
    tf
}

#[tokio::test]
async fn client_credentials_grant_body_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=client_credentials"))
        .and(body_string_contains("client_id=my-client"))
        .and(body_string_contains("client_secret=hunter2"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "atk",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/api/resources/org1/journey/actions/list"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!([])))
        .expect(1)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
designer:
  envs:
    dev:
      regions:
        eu: {{ base_url: {base}/api }}
      auth:
        grant_type: client_credentials
        token_url: {base}/token
        client_id_env: GGO_TEST_CLIENT_ID_CLIENT_CREDENTIALS_GRANT_BODY_SHAPE
        client_secret_env: TEST_CC_SECRET
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);

    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "dev",
            "--var",
            "orgId=org1",
            "designer",
            "journey-list",
        ])
        .env(
            "GGO_TEST_CLIENT_ID_CLIENT_CREDENTIALS_GRANT_BODY_SHAPE",
            "my-client",
        )
        .env("TEST_CC_SECRET", "hunter2")
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn password_grant_body_shape() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .and(body_string_contains("grant_type=password"))
        .and(body_string_contains("username=alice"))
        .and(body_string_contains("password=s3cret"))
        .and(body_string_contains("client_secret=cs-shh"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "atk",
            "expires_in": 3600
        })))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth:
        grant_type: password
        token_url: {base}/token
        client_id_env: GGO_TEST_CLIENT_ID_PASSWORD_GRANT_BODY_SHAPE
        client_secret_env: GGO_TEST_CLIENT_SECRET_PASSWORD_GRANT_BODY_SHAPE
        username_env: TEST_USER
        password_env: TEST_PASS
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "captain",
            "journey-state-fetch",
        ])
        .env("GGO_TEST_CLIENT_ID_PASSWORD_GRANT_BODY_SHAPE", "my-client")
        .env("GGO_TEST_CLIENT_SECRET_PASSWORD_GRANT_BODY_SHAPE", "cs-shh")
        .env("TEST_USER", "alice")
        .env("TEST_PASS", "s3cret")
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn bearer_override_skips_oauth() {
    let server = MockServer::start().await;
    // Token endpoint must NOT be hit.
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth:
        grant_type: client_credentials
        token_url: {base}/token
        client_id_env: GGO_TEST_CLIENT_ID_BEARER_OVERRIDE_SKIPS_OAUTH
        client_secret_env: TEST_SECRET
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn ggo_bearer_token_env_skips_oauth() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(500))
        .expect(0)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth:
        grant_type: client_credentials
        token_url: {base}/token
        client_id_env: GGO_TEST_CLIENT_ID_GGO_BEARER_TOKEN_ENV_SKIPS_OAUTH
        client_secret_env: TEST_SECRET
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "captain",
            "journey-state-fetch",
        ])
        .env("GGO_BEARER_TOKEN", jwt)
        .assert()
        .success();
}

#[tokio::test]
async fn expired_bearer_exits_3() {
    let cfg_yaml = r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: { base_urls: { v2: http://example } }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id_env: GGO_TEST_CLIENT_ID_EXPIRED_BEARER_EXITS_3
        client_secret_env: FOO
"#;
    let cfg = write_config(cfg_yaml);
    let expired = common::make_jwt(-3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            &expired,
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(3)
        .stderr(contains("expired"));
}

#[tokio::test]
async fn non_jwt_token_exits_2() {
    let cfg_yaml = r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: { base_urls: { v2: http://example } }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id_env: GGO_TEST_CLIENT_ID_NON_JWT_TOKEN_EXITS_2
        client_secret_env: FOO
"#;
    let cfg = write_config(cfg_yaml);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            "notajwt",
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("3 segments"));
}

#[tokio::test]
async fn jwe_token_rejected() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let cfg_yaml = r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: { base_urls: { v2: http://example } }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id_env: GGO_TEST_CLIENT_ID_JWE_TOKEN_REJECTED
        client_secret_env: FOO
"#;
    let cfg = write_config(cfg_yaml);
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            "a.b.c.d.e",
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(3)
        .stderr(contains("JWE"));
}

#[tokio::test]
async fn oauth_token_url_does_not_follow_302_redirect() {
    let server = MockServer::start().await;
    // Token URL returns 302; the CLI must NOT follow it (secrets protection).
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(
            ResponseTemplate::new(302).insert_header("Location", "http://attacker.invalid/relay"),
        )
        .expect(1)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth:
        grant_type: client_credentials
        token_url: {base}/token
        client_id_env: GGO_TEST_CLIENT_ID_OAUTH_TOKEN_URL_DOES_NOT_FOLLOW_302_REDIRECT
        client_secret_env: TEST_SECRET
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "captain",
            "journey-state-fetch",
        ])
        .env(
            "GGO_TEST_CLIENT_ID_OAUTH_TOKEN_URL_DOES_NOT_FOLLOW_302_REDIRECT",
            "my-client",
        )
        .env("TEST_SECRET", "hunter2")
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(3)
        .stderr(contains("redirect"));
}

#[tokio::test]
async fn expires_in_zero_rejected() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "access_token": "atk",
            "expires_in": 0
        })))
        .expect(1)
        .mount(&server)
        .await;
    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth:
        grant_type: client_credentials
        token_url: {base}/token
        client_id_env: GGO_TEST_CLIENT_ID_EXPIRES_IN_ZERO_REJECTED
        client_secret_env: TEST_SECRET
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "captain",
            "journey-state-fetch",
        ])
        .env("GGO_TEST_CLIENT_ID_EXPIRES_IN_ZERO_REJECTED", "my-client")
        .env("TEST_SECRET", "hunter2")
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(3)
        .stderr(contains("expires_in"));
}

#[tokio::test]
async fn missing_client_secret_env_exits_3() {
    let cfg_yaml = r#"
designer:
  envs:
    demo:
      regions:
        eu: { base_url: http://example/api }
      auth:
        grant_type: client_credentials
        token_url: http://example/token
        client_id_env: GGO_TEST_CLIENT_ID_MISSING_CLIENT_SECRET_ENV_EXITS_3
        client_secret_env: NEVER_SET_FOR_TEST
"#;
    let cfg = write_config(cfg_yaml);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "orgId=o",
            "designer",
            "journey-list",
        ])
        .env(
            "GGO_TEST_CLIENT_ID_MISSING_CLIENT_SECRET_ENV_EXITS_3",
            "foo",
        )
        .env_remove("GGO_BEARER_TOKEN")
        .env_remove("NEVER_SET_FOR_TEST")
        .assert()
        .code(3)
        .stderr(contains("NEVER_SET_FOR_TEST"));
}

#[tokio::test]
async fn exp_accepted_as_float() {
    // exp encoded as float (e.g. 1700000000.0).
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth: null
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let exp_f = (now + 3600) as f64;
    let jwt = common::make_jwt_with_raw_exp(serde_json::json!(exp_f));
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn exp_accepted_as_string() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;
    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth: null
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let jwt = common::make_jwt_with_raw_exp(serde_json::json!((now + 3600).to_string()));
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}
