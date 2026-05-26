//! Describe / list-endpoints JSON contract sanity checks.

use assert_cmd::Command;
use predicates::str::contains;

#[test]
fn describe_versioned_endpoint_emits_versions_block() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["describe", "captain", "journey-start", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("must be JSON");
    assert_eq!(v["schema_version"], "1");
    assert_eq!(v["service"], "captain");
    assert_eq!(v["name"], "journey-start");
    let supported = v["supported_versions"].as_array().expect("array");
    let names: Vec<&str> = supported.iter().map(|x| x.as_str().unwrap()).collect();
    assert!(
        names.contains(&"v1") && names.contains(&"v2"),
        "got {names:?}"
    );
    assert_eq!(v["versions"]["v1"]["method"], "POST");
    assert_eq!(v["versions"]["v2"]["method"], "POST");
    let v1_pattern = v["versions"]["v1"]["target_url_pattern"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(v1_pattern.contains("<base_url:v1>"), "got: {v1_pattern}");
    let v2_pattern = v["versions"]["v2"]["target_url_pattern"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(v2_pattern.contains("<base_url:v2>"), "got: {v2_pattern}");
}

#[test]
fn describe_disabled_endpoint_carries_reason() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["describe", "designer", "archive-journey", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("must be JSON");
    assert_eq!(v["disabled"], true);
    let reason = v["disabled_reason"].as_str().expect("reason present");
    assert!(
        reason.contains("pending verification"),
        "reason should mention pending verification; got: {reason}"
    );
}

#[test]
fn describe_human_readable_shows_status_line() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    // Disabled flat endpoint → human output shows "Status: disabled (...)".
    let out = Command::new(bin)
        .args(["describe", "designer", "archive-journey"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("Status: disabled"),
        "expected `Status: disabled` line, got: {stdout}"
    );

    // Enabled versioned endpoint → human output shows "Status: enabled".
    let out = Command::new(bin)
        .args(["describe", "captain", "version"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(
        stdout.contains("Status: enabled"),
        "expected `Status: enabled` line, got: {stdout}"
    );
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

#[test]
fn list_endpoints_captain_carries_supported_versions() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args(["list-endpoints", "--service", "captain", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = v.as_array().unwrap();
    assert!(!arr.is_empty(), "captain should have endpoints");
    // Every captain entry must have a supported_versions array.
    for entry in arr {
        assert!(
            entry.get("supported_versions").is_some(),
            "captain entry missing supported_versions: {entry}"
        );
    }
    // journey-start should report [v1, v2]; version should report [v2].
    let start = arr
        .iter()
        .find(|e| e["name"] == "journey-start")
        .expect("journey-start");
    let svs: Vec<&str> = start["supported_versions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_str().unwrap())
        .collect();
    assert!(svs.contains(&"v1") && svs.contains(&"v2"));
}

#[test]
fn list_endpoints_version_filter() {
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "list-endpoints",
            "--service",
            "captain",
            "--api-version",
            "v1",
            "--json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).unwrap();
    let arr = v.as_array().unwrap();
    // Every returned entry must support v1.
    for entry in arr {
        let svs: Vec<&str> = entry["supported_versions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect();
        assert!(svs.contains(&"v1"), "filter expected v1, entry: {entry}");
    }
}
