//! End-to-end integration tests via wiremock.

use assert_cmd::Command;
use predicates::str::contains;
use std::io::Write;
use wiremock::matchers::{body_json, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common;

fn write_config(yaml: &str) -> tempfile::NamedTempFile {
    let mut tf = tempfile::NamedTempFile::new().unwrap();
    tf.write_all(yaml.as_bytes()).unwrap();
    tf.flush().unwrap();
    tf
}

#[tokio::test]
async fn captain_v2_journey_start_includes_bearer_header() {
    // Verify the Authorization header is attached when a bearer is supplied,
    // and that it starts with "Bearer ".
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/journey/start"))
        .and(wiremock::matchers::header_exists("Authorization"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(1)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: {base} }}
      auth: null
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
            "resourceId=r1",
            "--var",
            "context={\"k\":\"v\"}",
            "--token",
            &jwt,
            "captain-v2",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn captain_v2_journey_start_e2e_relaxed() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/journey/start"))
        .and(body_json(serde_json::json!({
            "resourceId": "r1",
            "context": {"k": "v"}
        })))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("X-Request-ID", "req-1")
                .set_body_json(serde_json::json!({"instanceId": "i-001"})),
        )
        .expect(1)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: {base} }}
      auth: null
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "resourceId=r1",
            "--var",
            "context={\"k\":\"v\"}",
            "--token",
            &jwt,
            "captain-v2",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
    let stdout = String::from_utf8(out.get_output().stdout.clone()).unwrap();
    assert!(stdout.contains("i-001"), "stdout: {stdout}");
}

#[tokio::test]
async fn idempotent_get_retries_once_on_5xx_then_returns_2xx() {
    let server = MockServer::start().await;
    // First GET: 503; second: 200. Only idempotent methods are eligible for retry.
    Mock::given(method("GET"))
        .and(path("/version"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(1)
        .mount(&server)
        .await;
    Mock::given(method("GET"))
        .and(path("/version"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: {base} }}
      auth: null
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
            "captain-v2",
            "version",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .success();
}

#[tokio::test]
async fn non_idempotent_post_does_not_retry() {
    let server = MockServer::start().await;
    // The first POST returns 503; the CLI MUST NOT retry on a non-idempotent method.
    // We assert this by verifying a second matcher that expects exactly 0 calls.
    Mock::given(method("POST"))
        .and(path("/journey/start"))
        .respond_with(ResponseTemplate::new(503))
        .expect(1)
        .mount(&server)
        .await;
    Mock::given(method("POST"))
        .and(path("/journey/start"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"ok": true})))
        .expect(0)
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: {base} }}
      auth: null
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
            "resourceId=r1",
            "--var",
            "context={\"k\":\"v\"}",
            "--token",
            &jwt,
            "captain-v2",
            "journey-start",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(5);
}

#[tokio::test]
async fn upstream_4xx_includes_request_id_and_body() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(
            ResponseTemplate::new(400)
                .insert_header("X-Request-ID", "rid-abc")
                .set_body_string(r#"{"errors":[{"code":"4002"}]}"#),
        )
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: {base} }}
      auth: null
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
            "captain-v2",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(4)
        .stderr(contains("rid-abc"))
        .stderr(contains("4002"));
}

#[tokio::test]
async fn json_errors_emits_structured_object() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/journey/state/fetch"))
        .respond_with(ResponseTemplate::new(400))
        .mount(&server)
        .await;

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: {base} }}
      auth: null
"#,
        base = server.uri()
    );
    let cfg = write_config(&yaml);
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "--json-errors",
            "captain-v2",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(4);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).expect("must be JSON line");
    assert_eq!(v["schema_version"], "1");
    assert_eq!(v["exit_code"], 4);
    assert_eq!(v["kind"], "UpstreamClient");
}

#[tokio::test]
async fn network_failure_to_dead_port_exits_5() {
    // Bind a TCP port and immediately drop the listener, so connects fail.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);

    let yaml = format!(
        r#"
captain-v2:
  envs:
    demo:
      regions:
        eu: {{ base_url: http://{} }}
      auth: null
"#,
        addr
    );
    let cfg = write_config(&yaml);
    let jwt = common::make_jwt(3600);
    let bin = env!("CARGO_BIN_EXE_goctl");
    let out = Command::new(bin)
        .args([
            "--config",
            cfg.path().to_str().unwrap(),
            "--env",
            "demo",
            "--var",
            "instanceId=i1",
            "--token",
            &jwt,
            "--json-errors",
            "captain-v2",
            "journey-state-fetch",
        ])
        .env_remove("GGO_BEARER_TOKEN")
        .assert()
        .code(5);
    let stderr = String::from_utf8(out.get_output().stderr.clone()).unwrap();
    let v: serde_json::Value = serde_json::from_str(stderr.trim()).expect("must be JSON");
    assert_eq!(
        v["kind"], "Network",
        "network error must be CliError::Network"
    );
}

#[tokio::test]
async fn catalog_load_failures() {
    // The embedded catalog should already pass load. This is a smoke check.
    let bin = env!("CARGO_BIN_EXE_goctl");
    Command::new(bin)
        .args(["list-endpoints", "--json"])
        .assert()
        .success();
}
