//! Catalog-load validation: undeclared vars, GET+body, etc.

use goctl::endpoints::{
    Auth, EndpointBody, EndpointDef, FlatEndpoint, OptionalVar, RequiredVar, VarKind,
};

/// Helpers for flat endpoints (now wrapped inside `EndpointBody::Flat`).
fn as_flat(def: &EndpointDef) -> &FlatEndpoint {
    match &def.body {
        EndpointBody::Flat(f) => f,
        EndpointBody::Versioned(_) => panic!("expected flat endpoint"),
    }
}

fn must_have_json_encode_on_string(template: &str, var: &str) -> bool {
    let mut i = 0usize;
    let bytes = template.as_bytes();
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                j += 1;
            }
            let inner = &template[start..j];
            let parts: Vec<&str> = inner.split('|').map(|s| s.trim()).collect();
            if parts
                .first()
                .map(|p| p.split_whitespace().next().unwrap_or(""))
                == Some(var)
                && parts.iter().skip(1).any(|f| {
                    f.split(|c: char| c == '(' || c.is_whitespace())
                        .next()
                        .unwrap_or("")
                        == "json_encode"
                })
            {
                return true;
            }
            i = j + 2;
        } else {
            i += 1;
        }
    }
    false
}

#[test]
fn manifest_yaml_parses_with_kind_field() {
    let y = r#"
name: t
description: test
method: POST
path: /t
auth: bearer
required_vars:
  - { name: x, kind: string, description: "x", example: "ex" }
body_template: |
  { "x": {{ x | json_encode }} }
"#;
    let def: EndpointDef = serde_yaml::from_str(y).expect("parse");
    let flat = as_flat(&def);
    assert_eq!(flat.required_vars[0].kind, VarKind::String);
    assert!(must_have_json_encode_on_string(
        flat.body_template.as_deref().unwrap(),
        "x"
    ));
}

#[test]
fn endpoint_def_serializes() {
    let d = EndpointDef {
        name: "t".into(),
        description: "x".into(),
        auth: Auth::None,
        body: EndpointBody::Flat(FlatEndpoint {
            method: "GET".into(),
            path: "/t".into(),
            required_vars: vec![RequiredVar {
                name: "a".into(),
                kind: VarKind::String,
                description: "a".into(),
                example: None,
            }],
            optional_vars: vec![OptionalVar {
                name: "b".into(),
                kind: VarKind::Json,
                description: "b".into(),
                default: Some("{}".into()),
            }],
            body_template: None,
            disabled: false,
            disabled_reason: None,
        }),
    };
    let s = serde_json::to_string(&d).unwrap();
    assert!(s.contains("\"kind\":\"string\""));
    assert!(s.contains("\"kind\":\"json\""));
    assert!(s.contains("\"auth\":\"none\""));
}

/// End-to-end check that the embedded catalog (used by `goctl list-endpoints`) succeeds.
#[test]
fn embedded_catalog_loads_clean() {
    let catalog = goctl::endpoints::Catalog::load_embedded().expect("catalog must load");
    let total: usize = catalog.services.values().map(|m| m.len()).sum();
    assert!(total >= 50, "expected ~57 manifests, got {total}");
    for svc in ["captain", "designer", "userview"] {
        assert!(catalog.services.contains_key(svc), "missing service {svc}");
    }
    // Captain is consolidated — there must be no `captain-v1` / `captain-v2` keys.
    assert!(!catalog.services.contains_key("captain-v1"));
    assert!(!catalog.services.contains_key("captain-v2"));
}

/// Verify embedded catalog meets all invariants per-version when versioned.
#[test]
fn embedded_catalog_invariants() {
    let catalog = goctl::endpoints::Catalog::load_embedded().unwrap();
    for (svc, eps) in &catalog.services {
        for (name, def) in eps {
            match &def.body {
                EndpointBody::Flat(f) => {
                    let m = f.method.to_ascii_uppercase();
                    if m == "GET" || m == "HEAD" {
                        assert!(
                            f.body_template.is_none(),
                            "{svc}/{name}: GET/HEAD must not have body_template"
                        );
                    }
                    assert!(
                        f.path.starts_with('/'),
                        "{svc}/{name}: path must start with /"
                    );
                }
                EndpointBody::Versioned(vb) => {
                    for (v, ve) in &vb.versions {
                        let m = ve.method.to_ascii_uppercase();
                        if m == "GET" || m == "HEAD" {
                            assert!(
                                ve.body_template.is_none(),
                                "{svc}/{name}@{v}: GET/HEAD must not have body_template"
                            );
                        }
                        assert!(
                            ve.path.starts_with('/'),
                            "{svc}/{name}@{v}: path must start with /"
                        );
                    }
                }
            }
        }
    }
}

/// All template refs in the embedded catalog must be declared vars.
#[test]
fn body_template_references_only_declared_vars() {
    let catalog = goctl::endpoints::Catalog::load_embedded().unwrap();
    for (svc, eps) in &catalog.services {
        for (name, def) in eps {
            match &def.body {
                EndpointBody::Flat(f) => {
                    if let Some(tmpl) = &f.body_template {
                        assert_refs(svc, name, None, tmpl, &f.required_vars, &f.optional_vars);
                    }
                }
                EndpointBody::Versioned(vb) => {
                    for (v, ve) in &vb.versions {
                        if let Some(tmpl) = &ve.body_template {
                            assert_refs(
                                svc,
                                name,
                                Some(v),
                                tmpl,
                                &ve.required_vars,
                                &ve.optional_vars,
                            );
                        }
                    }
                }
            }
        }
    }
}

fn assert_refs(
    svc: &str,
    name: &str,
    version: Option<&str>,
    tmpl: &str,
    req: &[RequiredVar],
    opt: &[OptionalVar],
) {
    let declared: std::collections::BTreeSet<&str> = req
        .iter()
        .map(|v| v.name.as_str())
        .chain(opt.iter().map(|v| v.name.as_str()))
        .collect();
    let mut i = 0usize;
    let bytes = tmpl.as_bytes();
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                j += 1;
            }
            let inner = &tmpl[start..j];
            let parts: Vec<&str> = inner.split('|').map(|s| s.trim()).collect();
            if let Some(first) = parts.first() {
                let var_name = first.split_whitespace().next().unwrap_or("");
                if !var_name.is_empty() {
                    assert!(
                        declared.contains(var_name),
                        "{svc}/{name}{}: body_template references undeclared var `{var_name}`",
                        version.map(|v| format!("@{v}")).unwrap_or_default()
                    );
                }
            }
            i = j + 2;
        } else {
            i += 1;
        }
    }
}

#[test]
fn manifest_get_with_body_template_rejected_by_validator() {
    let yaml = r#"
name: bad
description: bad
method: GET
path: /t
auth: none
body_template: "{}"
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(err.to_string().contains("body_template"), "got: {err}");
}

#[test]
fn manifest_undeclared_var_reference_rejected_by_validator() {
    let yaml = r#"
name: bad
description: bad
method: POST
path: /t
auth: none
required_vars:
  - { name: x, kind: string, description: x }
body_template: |
  { "x": {{ x | json_encode }}, "y": {{ y | json_encode }} }
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(err.to_string().contains("undeclared var"), "got: {err}");
}

#[test]
fn manifest_string_var_without_json_encode_rejected() {
    let yaml = r#"
name: bad
description: bad
method: POST
path: /t
auth: none
required_vars:
  - { name: x, kind: string, description: x }
body_template: |
  { "x": "{{ x }}" }
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(err.to_string().contains("json_encode"), "got: {err}");
}

#[test]
fn manifest_invalid_method_rejected() {
    let yaml = r#"
name: bad
description: bad
method: FOO
path: /t
auth: none
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(err.to_string().contains("invalid method"), "got: {err}");
}

#[test]
fn manifest_path_without_leading_slash_rejected() {
    let yaml = r#"
name: bad
description: bad
method: GET
path: relative/path
auth: none
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(err.to_string().contains("start with"), "got: {err}");
}

/// Versioned manifest where `supported_versions` lists a version that is not in
/// `versions:` must be rejected.
#[test]
fn versioned_manifest_orphan_supported_version_rejected() {
    let yaml = r#"
name: bad
description: bad
auth: bearer
supported_versions: [v1, v2]
versions:
  v1:
    method: POST
    path: /t
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(err.to_string().contains("no `versions.v2`"), "got: {err}");
}

/// Versioned manifest with a `versions:` block not listed in `supported_versions`
/// must be rejected.
#[test]
fn versioned_manifest_orphan_version_block_rejected() {
    let yaml = r#"
name: bad
description: bad
auth: bearer
supported_versions: [v1]
versions:
  v1: { method: POST, path: /t }
  v2: { method: POST, path: /t }
"#;
    let def: EndpointDef = serde_yaml::from_str(yaml).unwrap();
    let err = goctl::endpoints::validate_endpoint(&def).unwrap_err();
    assert_eq!(err.kind.code(), 1);
    assert!(
        err.to_string().contains("not in supported_versions"),
        "got: {err}"
    );
}
