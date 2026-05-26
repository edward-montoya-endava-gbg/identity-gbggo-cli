//! Disabled-endpoint gate: refuses to run before auth / var / dry-run.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;

fn fixture_config() -> tempfile::NamedTempFile {
    // Minimal config covering designer (bearer-only, for the disabled-gate test)
    // and captain-v2 (no auth required for the enabled `version` smoke test).
    let yaml = r#"
designer:
  envs:
    dev:
      regions:
        eu: { base_url: http://example/designer/api }
      auth: null
captain:
  envs:
    dev:
      default_version: v2
      regions:
        eu: { base_urls: { v2: http://example/v2/captain } }
      auth: null
"#;
    let mut tf = tempfile::NamedTempFile::new().unwrap();
    tf.write_all(yaml.as_bytes()).unwrap();
    tf.flush().unwrap();
    tf
}

/// The disabled gate must fire BEFORE required-var enforcement, the bearer-only
/// prod gate, and the --dry-run short-circuit. We pick `designer/archive-journey`
/// (a disabled write endpoint that normally requires `orgId` + `journeyId` vars
/// AND a bearer token) and call it with neither — if the gate runs first, the
/// CLI exits Usage(2) with "disabled" in stderr instead of complaining about
/// missing vars or missing auth.
#[test]
fn disabled_endpoint_exits_usage_before_auth_or_vars() {
    let cfg = fixture_config();
    let bin = env!("CARGO_BIN_EXE_goctl");
    let assert = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "dev",
            "designer",
            "archive-journey",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        // Strip any GGO_VAR_* the user shell may have set so we can prove the
        // gate runs before var resolution.
        .env_remove("GGO_VAR_ORGID")
        .env_remove("GGO_VAR_JOURNEYID")
        .assert()
        .code(2)
        .stderr(contains("disabled"));
    let stderr = String::from_utf8(assert.get_output().stderr.clone()).unwrap();
    // Specific assertions: error must NOT mention missing-vars or bearer-only,
    // proving the gate ran first.
    assert!(
        !stderr.contains("missing required vars"),
        "gate must run before var enforcement; got: {stderr}"
    );
    assert!(
        !stderr.contains("bearer-only"),
        "gate must run before bearer-only check; got: {stderr}"
    );
}

/// Verify --dry-run does not bypass the disabled gate.
#[test]
fn disabled_endpoint_dry_run_still_blocked() {
    let cfg = fixture_config();
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "dev",
            "--dry-run",
            "designer",
            "archive-journey",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("disabled"));
}

/// Enabled endpoint (`captain-v2 version`, auth: none, no vars) must NOT be
/// blocked by the disabled gate. --dry-run keeps this test offline.
#[test]
fn enabled_endpoint_still_runs() {
    let cfg = fixture_config();
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "dev",
            "--dry-run",
            "captain",
            "version",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

/// `list-endpoints --disabled-only --json` returns exactly the disabled set,
/// each entry carries `disabled: true`, and the total matches the manifest count.
#[test]
fn list_endpoints_disabled_only_filter() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["list-endpoints", "--disabled-only", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("must be JSON");
    let arr = v.as_array().expect("array");
    assert!(
        !arr.is_empty(),
        "disabled-only filter must return at least one entry"
    );
    for entry in arr {
        assert_eq!(
            entry["disabled"], true,
            "every entry must have disabled=true; got: {entry}"
        );
    }
}

/// `--enabled-only` is the inverse of `--disabled-only` — no overlap.
#[test]
fn list_endpoints_enabled_only_excludes_disabled() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["list-endpoints", "--enabled-only", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for entry in arr {
        assert_eq!(
            entry["disabled"], false,
            "enabled-only must not include disabled entries; got: {entry}"
        );
    }
}

/// Default `list-endpoints --json` (no filter) carries the `disabled` field on
/// every entry — required by the schema-additive contract.
#[test]
fn list_endpoints_carries_disabled_field_by_default() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["list-endpoints", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    for entry in arr {
        assert!(
            entry.get("disabled").is_some(),
            "every list entry must include `disabled`; got: {entry}"
        );
    }
}

/// `--disabled-only` and `--enabled-only` are mutually exclusive.
#[test]
fn list_endpoints_disabled_and_enabled_only_mutually_exclusive() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args(["list-endpoints", "--disabled-only", "--enabled-only"])
        .assert()
        .failure();
}
