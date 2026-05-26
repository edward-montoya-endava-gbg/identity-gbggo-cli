//! Fixture catalog + `--include` overlay + `--var key=@<path>` loader tests.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;

mod common;

/// Helper: write a regions.yaml that maps captain v2 at the given (mock or real)
/// base URL. The tests below never make a network call — they all use
/// `--dry-run` which prints the rendered request to stdout.
fn write_regions(base: &str) -> tempfile::NamedTempFile {
    let yaml = format!(
        r#"
captain:
  envs:
    demo:
      default_version: v2
      regions:
        eu: {{ base_urls: {{ v2: {base} }} }}
      auth: null
"#
    );
    let mut tf = tempfile::NamedTempFile::new().unwrap();
    tf.write_all(yaml.as_bytes()).unwrap();
    tf.flush().unwrap();
    tf
}

/// Pull out the JSON body from a `--dry-run` output. `print_dry_run` writes:
///   METHOD URL
///   [Authorization: …]
///
///   <body...>
fn parse_dry_run_body(stdout: &str) -> serde_json::Value {
    let body_start = stdout.find('{').expect("dry-run output missing JSON body");
    let body = &stdout[body_start..];
    serde_json::from_str(body).expect("rendered body must be valid JSON")
}

#[test]
fn built_in_identity_fixture_loads_with_no_file_refs() {
    let v = goctl::fixtures::resolve("identity", "testdata-v1").expect("loads");
    assert_eq!(v["firstName"], "Sean");
    assert_eq!(v["lastName"], "Martin");
    assert_eq!(v["address"]["country"], "GB");
}

#[test]
fn biometrics_fixture_inlines_image_files() {
    let v = goctl::fixtures::resolve("biometrics", "default").expect("loads");
    let selfie = v["selfieImage"].as_str().unwrap();
    let anchor = v["anchorImage"].as_str().unwrap();
    assert!(!selfie.is_empty(), "selfieImage should not be empty");
    assert!(!anchor.is_empty(), "anchorImage should not be empty");
    assert!(
        !selfie.starts_with("@file:"),
        "selfieImage should be inlined, got: {selfie}"
    );
    assert!(
        !anchor.starts_with("@file:"),
        "anchorImage should be inlined, got: {anchor}"
    );
    // Placeholder content sanity check.
    assert!(
        selfie.contains("REPLACE_WITH_BASE64"),
        "expected placeholder; got: {selfie}"
    );
}

#[test]
fn include_flag_overlays_context_for_captain_journey_start() {
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--include",
            "identity",
            "testdata-v1",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    assert_eq!(body["context"]["identity"]["firstName"], "Sean");
}

#[test]
fn multiple_includes_merge_into_context() {
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--include",
            "identity",
            "testdata-v1",
            "--include",
            "biometrics",
            "default",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    assert_eq!(body["context"]["identity"]["firstName"], "Sean");
    assert!(
        body["context"]["biometrics"]["selfieImage"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "biometrics.selfieImage should be a non-empty string"
    );
}

#[test]
fn user_var_context_provides_base_for_includes() {
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--var",
            r#"context={"locale":"en-US"}"#,
            "--include",
            "identity",
            "testdata-v1",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    assert_eq!(body["context"]["locale"], "en-US");
    assert_eq!(body["context"]["identity"]["firstName"], "Sean");
}

#[test]
fn user_passes_full_context_json_without_includes_works() {
    // Spec reinforcement: raw `--var context='<json>'` MUST remain the canonical
    // path. With NO `--include` flags, the rendered `context` MUST equal the
    // user-supplied JSON byte-for-byte (modulo whitespace).
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let raw_context = r#"{"identity":{"firstName":"X"},"locale":"en-US"}"#;
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--var",
            &format!("context={raw_context}"),
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    let expected: serde_json::Value = serde_json::from_str(raw_context).unwrap();
    assert_eq!(
        body["context"], expected,
        "rendered context must match supplied JSON; got: {}",
        body["context"]
    );
}

#[test]
fn include_with_path_falls_back_to_filesystem() {
    let mut tf = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
    writeln!(tf, "firstName: \"FromDisk\"").unwrap();
    writeln!(tf, "lastName: \"User\"").unwrap();
    tf.flush().unwrap();
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    // The temp file path contains `.` and `/` so the loader takes the
    // filesystem branch.
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--include",
            "identity",
            tf.path().to_str().unwrap(),
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    assert_eq!(body["context"]["identity"]["firstName"], "FromDisk");
}

#[test]
fn unknown_fixture_exits_usage() {
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--include",
            "identity",
            "nonexistent",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    assert!(
        stderr.contains("testdata-v1") || stderr.contains("testdata-v2"),
        "stderr must enumerate available identity fixtures; got: {stderr}"
    );
}

#[test]
fn list_fixtures_returns_built_in_set() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["list-fixtures", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("must be JSON");
    let arr = v.as_array().expect("array");
    let names: Vec<(String, String)> = arr
        .iter()
        .map(|e| {
            (
                e["category"].as_str().unwrap().to_string(),
                e["name"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    // 2 identity + 2 documents + 1 biometrics = 5 expected.
    assert_eq!(arr.len(), 5, "expected 5 fixtures, got {arr:?}");
    assert!(names.contains(&("identity".into(), "testdata-v1".into())));
    assert!(names.contains(&("identity".into(), "testdata-v2".into())));
    assert!(names.contains(&("documents".into(), "testdata-v1".into())));
    assert!(names.contains(&("documents".into(), "testdata-v2".into())));
    assert!(names.contains(&("biometrics".into(), "default".into())));
}

#[test]
fn describe_fixture_inlines_image_references() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["describe-fixture", "biometrics", "default", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("must be JSON");
    let selfie = v["selfieImage"].as_str().unwrap();
    assert!(
        !selfie.starts_with("@file:"),
        "selfieImage should be inlined, got: {selfie}"
    );
    assert!(
        selfie.contains("REPLACE_WITH_BASE64"),
        "expected placeholder content; got: {selfie}"
    );
}

#[test]
fn var_with_at_prefix_loads_json_from_file() {
    let mut tf = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    writeln!(tf, r#"{{"identity":{{"firstName":"FromFile"}}}}"#).unwrap();
    tf.flush().unwrap();

    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--var",
            &format!("context=@{}", tf.path().to_str().unwrap()),
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    assert_eq!(body["context"]["identity"]["firstName"], "FromFile");
}

#[test]
fn var_with_at_prefix_loads_yaml_from_file() {
    let mut tf = tempfile::Builder::new().suffix(".yaml").tempfile().unwrap();
    writeln!(tf, "identity:").unwrap();
    writeln!(tf, "  firstName: FromYAML").unwrap();
    writeln!(tf, "locale: en-GB").unwrap();
    tf.flush().unwrap();

    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--var",
            &format!("context=@{}", tf.path().to_str().unwrap()),
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let body = parse_dry_run_body(&stdout);
    assert_eq!(body["context"]["identity"]["firstName"], "FromYAML");
    assert_eq!(body["context"]["locale"], "en-GB");
}

#[test]
fn var_at_prefix_missing_file_exits_usage() {
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--var",
            "context=@/nonexistent/definitely/not/here.json",
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("file not found"));
}

#[test]
fn var_at_prefix_invalid_json_exits_usage() {
    let mut tf = tempfile::Builder::new().suffix(".json").tempfile().unwrap();
    writeln!(tf, "{{not valid json").unwrap();
    tf.flush().unwrap();
    let cfg = write_regions("http://127.0.0.1:1");
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--api-version",
            "v2",
            "--var",
            "resourceId=r1",
            "--var",
            &format!("context=@{}", tf.path().to_str().unwrap()),
            "--token",
            &jwt,
            "--dry-run",
            "captain",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(2)
        .stderr(contains("not valid JSON"));
}
