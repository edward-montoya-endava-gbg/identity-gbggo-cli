//! Bearer-only prod gate matrix.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;

mod common;

fn fixture_config() -> tempfile::NamedTempFile {
    let yaml = r#"
designer:
  envs:
    prod:
      regions:
        eu: { base_url: http://example/designer/api }
      auth: null
captain:
  envs:
    prod:
      default_version: v2
      regions:
        eu: { base_urls: { v2: http://example/v2/captain } }
      auth:
        grant_type: password
        token_url: http://example/oauth2/token
        client_id_env: GGO_TEST_PROD_GATE_CLIENT_ID
        client_secret_env: GGO_TEST_PROD_GATE_CLIENT_SECRET
        username_env: GGO_CAPTAIN_V2_PROD_USERNAME
        password_env: GGO_CAPTAIN_V2_PROD_PASSWORD
userview:
  envs:
    prod:
      regions:
        eu: { base_url: http://example/api }
      auth: null
"#;
    let mut tf = tempfile::NamedTempFile::new().unwrap();
    tf.write_all(yaml.as_bytes()).unwrap();
    tf.flush().unwrap();
    tf
}

#[test]
fn designer_prod_without_bearer_exits_2() {
    let cfg = fixture_config();
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--var",
            "orgId=org1",
            "designer",
            "journey-list",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("bearer-only"));
}

#[test]
fn userview_prod_without_bearer_exits_2() {
    let cfg = fixture_config();
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--region",
            "eu",
            "--var",
            "orgId=org1",
            "userview",
            "journey-sessions-list",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("bearer-only"));
}

#[test]
fn designer_prod_with_bearer_proceeds() {
    let cfg = fixture_config();
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--var",
            "orgId=org1",
            "--token",
            &jwt,
            "--dry-run",
            "designer",
            "journey-list",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success()
        .stdout(contains("Authorization: Bearer ***"));
}

#[test]
fn auth_none_endpoint_in_prod_reaches_upstream_without_bearer() {
    // userview/health declares auth: none — must skip both the bearer-only gate and OAuth.
    let cfg = fixture_config();
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--region",
            "eu",
            "--dry-run",
            "userview",
            "health",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        !stdout.contains("Authorization:"),
        "auth: none endpoint must not include any Authorization header line, got: {stdout}"
    );
}

#[test]
fn captain_v2_prod_write_without_confirm_exits_2() {
    let cfg = fixture_config();
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--region",
            "eu",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "captain",
            "journey-state-delete",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("confirm"));
}

#[test]
fn captain_v2_prod_write_with_dry_run_no_confirm_ok() {
    // --dry-run waives --confirm.
    let cfg = fixture_config();
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--region",
            "eu",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-state-delete",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[test]
fn token_url_on_bearer_only_target_is_rejected() {
    let cfg = fixture_config();
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--var",
            "orgId=org1",
            "--token",
            &jwt,
            "--token-url",
            "http://override/token",
            "--dry-run",
            "designer",
            "journey-list",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains(
            "--token-url has no effect when --token is supplied",
        ));
}

#[test]
fn duplicate_env_flag_rejected() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--env",
            "dev",
            "--env",
            "prod",
            "designer",
            "journey-list",
            "--var",
            "orgId=o",
        ])
        .assert()
        .code(2)
        .stderr(contains("more than once"));
}

#[test]
fn region_case_insensitive() {
    let cfg = fixture_config();
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "prod",
            "--region",
            "EU",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}
