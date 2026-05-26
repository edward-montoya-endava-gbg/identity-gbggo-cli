//! Body template rendering, var precedence, JSON safety.

use goctl::endpoints::{EndpointView, OptionalVar, RequiredVar, VarKind};
use goctl::render::{render_body, resolve_vars, substitute_path};
use std::collections::BTreeMap;

fn make_view<'a>(
    body: Option<&'a str>,
    method: &'a str,
    path: &'a str,
    req: &'a [RequiredVar],
    opt: &'a [OptionalVar],
) -> EndpointView<'a> {
    EndpointView {
        method,
        path,
        required_vars: req,
        optional_vars: opt,
        body_template: body,
        disabled: false,
        disabled_reason: None,
        deprecated: None,
        version: None,
    }
}

#[test]
fn string_var_with_quote_and_backslash_produces_valid_json() {
    let req = vec![RequiredVar {
        name: "x".into(),
        kind: VarKind::String,
        description: "x".into(),
        example: None,
    }];
    let opt: Vec<OptionalVar> = vec![];
    let view = make_view(
        Some(r#"{"x": {{ x | json_encode }}}"#),
        "POST",
        "/t",
        &req,
        &opt,
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("x".into(), "a\"b\\nc\nd".into());
    let resolved = resolve_vars(&view, &cli_vars).unwrap();
    let body = render_body(&view, &resolved).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).expect("must be valid JSON");
    assert_eq!(v["x"].as_str().unwrap(), "a\"b\\nc\nd");
}

#[test]
fn json_var_pre_validated_before_render() {
    let req = vec![RequiredVar {
        name: "y".into(),
        kind: VarKind::Json,
        description: "y".into(),
        example: None,
    }];
    let opt: Vec<OptionalVar> = vec![];
    let view = make_view(
        Some(r#"{"y": {{ y | json_encode | safe }}}"#),
        "POST",
        "/t",
        &req,
        &opt,
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("y".into(), "not json".into());
    let err = resolve_vars(&view, &cli_vars).unwrap_err();
    assert!(err.to_string().contains("not valid JSON"), "got: {err}");
    assert_eq!(err.kind.code(), 2, "must exit 2 (Usage) before HTTP/auth");
}

#[test]
fn required_var_missing_exits_usage() {
    let req = vec![RequiredVar {
        name: "x".into(),
        kind: VarKind::String,
        description: "x".into(),
        example: None,
    }];
    let opt: Vec<OptionalVar> = vec![];
    let view = make_view(
        Some(r#"{"x": {{ x | json_encode }}}"#),
        "POST",
        "/t",
        &req,
        &opt,
    );
    let cli_vars = BTreeMap::new();
    let err = resolve_vars(&view, &cli_vars).unwrap_err();
    assert!(
        err.to_string().contains("missing required vars"),
        "got: {err}"
    );
    assert_eq!(err.kind.code(), 2);
}

#[test]
fn cli_var_overrides_default() {
    let req: Vec<RequiredVar> = vec![];
    let opt = vec![OptionalVar {
        name: "x".into(),
        kind: VarKind::String,
        description: "x".into(),
        default: Some("from-default".into()),
    }];
    let view = make_view(
        Some(r#"{"x": {{ x | json_encode }}}"#),
        "POST",
        "/t",
        &req,
        &opt,
    );
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("x".into(), "from-cli".into());
    let resolved = resolve_vars(&view, &cli_vars).unwrap();
    let body = render_body(&view, &resolved).unwrap().unwrap();
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["x"].as_str().unwrap(), "from-cli");
}

#[test]
fn env_var_overrides_default() {
    let req: Vec<RequiredVar> = vec![];
    let opt = vec![OptionalVar {
        name: "foo_bar".into(),
        kind: VarKind::String,
        description: "test".into(),
        default: Some("from-default".into()),
    }];
    let view = make_view(
        Some(r#"{"foo_bar": {{ foo_bar | json_encode }}}"#),
        "POST",
        "/t",
        &req,
        &opt,
    );
    std::env::set_var("GGO_VAR_FOO_BAR", "from-env");
    let cli_vars = BTreeMap::new();
    let resolved = resolve_vars(&view, &cli_vars).unwrap();
    let body = render_body(&view, &resolved).unwrap().unwrap();
    std::env::remove_var("GGO_VAR_FOO_BAR");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["foo_bar"].as_str().unwrap(), "from-env");
}

#[test]
fn path_var_with_ampersand_is_percent_encoded() {
    let req = vec![RequiredVar {
        name: "orgId".into(),
        kind: VarKind::String,
        description: "org".into(),
        example: None,
    }];
    let opt: Vec<OptionalVar> = vec![];
    let view = make_view(None, "GET", "/{orgId}/journey/revisions", &req, &opt);
    let mut cli_vars = BTreeMap::new();
    cli_vars.insert("orgId".into(), "foo&bar=baz".into());
    let resolved = resolve_vars(&view, &cli_vars).unwrap();
    let path = substitute_path("/{orgId}/journey/revisions", &resolved).unwrap();
    assert!(
        path.contains("foo%26bar%3Dbaz"),
        "expected percent-encoded `&` and `=`, got: {path}"
    );
    assert!(!path.contains('&'), "`&` must not appear raw in: {path}");
    assert!(!path.contains('='), "`=` must not appear raw in: {path}");
}
