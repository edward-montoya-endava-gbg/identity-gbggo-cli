//! Body template rendering, var precedence, JSON safety.

use goctl::endpoints::{Auth, EndpointDef, OptionalVar, RequiredVar, VarKind};
use goctl::render::{render_body, resolve_vars, substitute_path};
use std::collections::BTreeMap;

fn make_def(body: Option<&str>, req: Vec<RequiredVar>, opt: Vec<OptionalVar>) -> EndpointDef {
    EndpointDef {
        name: "t".into(),
        description: "test".into(),
        method: "POST".into(),
        path: "/t".into(),
        auth: Auth::Bearer,
        required_vars: req,
        optional_vars: opt,
        body_template: body.map(String::from),
    }
}

#[test]
fn string_var_with_quote_and_backslash_produces_valid_json() {
    let def = make_def(
        Some(r#"{"x": {{ x | json_encode }}}"#),
        vec![RequiredVar {
            name: "x".into(),
            kind: VarKind::String,
            description: "x".into(),
            example: None,
        }],
        vec![],
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("x".into(), "a\"b\\nc\nd".into());
    let resolved = resolve_vars(&def, &cli_vars).unwrap();
    let body = render_body(&def, &resolved).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).expect("must be valid JSON");
    assert_eq!(v["x"].as_str().unwrap(), "a\"b\\nc\nd");
}

#[test]
fn json_var_pre_validated_before_render() {
    let def = make_def(
        Some(r#"{"y": {{ y | json_encode | safe }}}"#),
        vec![RequiredVar {
            name: "y".into(),
            kind: VarKind::Json,
            description: "y".into(),
            example: None,
        }],
        vec![],
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("y".into(), "not json".into());
    let err = resolve_vars(&def, &cli_vars).unwrap_err();
    assert!(err.to_string().contains("not valid JSON"), "got: {err}");
    assert_eq!(err.kind.code(), 2, "must exit 2 (Usage) before HTTP/auth");
}

#[test]
fn required_var_missing_exits_usage() {
    let def = make_def(
        Some(r#"{"x": {{ x | json_encode }}}"#),
        vec![RequiredVar {
            name: "x".into(),
            kind: VarKind::String,
            description: "x".into(),
            example: None,
        }],
        vec![],
    );
    let cli_vars = BTreeMap::new();
    let err = resolve_vars(&def, &cli_vars).unwrap_err();
    assert!(
        err.to_string().contains("missing required vars"),
        "got: {err}"
    );
    assert_eq!(err.kind.code(), 2);
}

#[test]
fn cli_var_overrides_default() {
    let def = make_def(
        Some(r#"{"x": {{ x | json_encode }}}"#),
        vec![],
        vec![OptionalVar {
            name: "x".into(),
            kind: VarKind::String,
            description: "x".into(),
            default: Some("from-default".into()),
        }],
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("x".into(), "from-cli".into());
    let resolved = resolve_vars(&def, &cli_vars).unwrap();
    let body = render_body(&def, &resolved).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["x"].as_str().unwrap(), "from-cli");
}

#[test]
fn env_var_overrides_default() {
    let def = make_def(
        Some(r#"{"foo_bar": {{ foo_bar | json_encode }}}"#),
        vec![],
        vec![OptionalVar {
            name: "foo_bar".into(),
            kind: VarKind::String,
            description: "test".into(),
            default: Some("from-default".into()),
        }],
    );
    // Set env var (must be safe across other tests since name is unique).
    std::env::set_var("GGO_VAR_FOO_BAR", "from-env");
    let cli_vars = BTreeMap::new();
    let resolved = resolve_vars(&def, &cli_vars).unwrap();
    let body = render_body(&def, &resolved).unwrap().unwrap();
    std::env::remove_var("GGO_VAR_FOO_BAR");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["foo_bar"].as_str().unwrap(), "from-env");
}

#[test]
fn path_var_with_ampersand_is_percent_encoded() {
    // A path var value containing URL-reserved sub-delims (`&`, `=`) must be
    // percent-encoded so it cannot leak into the query string.
    let def = make_def(
        None,
        vec![RequiredVar {
            name: "orgId".into(),
            kind: VarKind::String,
            description: "org".into(),
            example: None,
        }],
        vec![],
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("orgId".into(), "foo&bar=baz".into());
    let resolved = resolve_vars(&def, &cli_vars).unwrap();
    let path = substitute_path("/{orgId}/journey/revisions", &resolved).unwrap();
    assert!(
        path.contains("foo%26bar%3Dbaz"),
        "expected percent-encoded `&` and `=`, got: {path}"
    );
    assert!(!path.contains('&'), "`&` must not appear raw in: {path}");
    assert!(!path.contains('='), "`=` must not appear raw in: {path}");
}
