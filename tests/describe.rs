//! Describe / list-endpoints JSON contract sanity checks.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn describe_emits_schema_v1_json() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["describe", "captain-v2", "journey-start", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("must be JSON");
    assert_eq!(v["schema_version"], "1");
    assert_eq!(v["service"], "captain-v2");
    assert_eq!(v["name"], "journey-start");
    assert_eq!(v["method"], "POST");
    assert!(v["target_url_pattern"]
        .as_str()
        .unwrap()
        .contains("/journey/start"));
}

#[test]
fn list_endpoints_emits_array() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["list-endpoints", "--service", "designer", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    assert!(v.is_array());
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty());
    assert!(arr.iter().all(|e| e["service"] == "designer"));
}

#[test]
fn list_endpoints_unknown_service_errors() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args(["list-endpoints", "--service", "nope", "--json"])
        .assert()
        .failure()
        .stderr(contains("not found"));
}
