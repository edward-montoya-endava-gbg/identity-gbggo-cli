//! Endpoint catalog: YAML manifests embedded at compile time via `include_dir!`.
//!
//! Validates manifests at startup:
//! - method in {GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS}
//! - path starts with `/`
//! - GET (and HEAD) endpoints have no `body_template`
//! - no intra-service name collisions
//! - every `{{ var }}` in `body_template` is declared in required_vars ∪ optional_vars
//! - every string-typed var reference in body_template uses `| json_encode`

use crate::error::{CliError, CliResult};
use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

static CATALOG_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/endpoints");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Auth {
    Bearer,
    None,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum VarKind {
    #[default]
    String,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RequiredVar {
    pub name: String,
    #[serde(default)]
    pub kind: VarKind,
    pub description: String,
    #[serde(default)]
    pub example: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OptionalVar {
    pub name: String,
    #[serde(default)]
    pub kind: VarKind,
    pub description: String,
    #[serde(default)]
    pub default: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EndpointDef {
    pub name: String,
    pub description: String,
    pub method: String,
    pub path: String,
    pub auth: Auth,
    #[serde(default)]
    pub required_vars: Vec<RequiredVar>,
    #[serde(default)]
    pub optional_vars: Vec<OptionalVar>,
    #[serde(default)]
    pub body_template: Option<String>,
}

impl EndpointDef {
    pub fn var_kind(&self, name: &str) -> Option<VarKind> {
        for r in &self.required_vars {
            if r.name == name {
                return Some(r.kind);
            }
        }
        for o in &self.optional_vars {
            if o.name == name {
                return Some(o.kind);
            }
        }
        None
    }
}

/// Service identifier — string-keyed for flexibility.
pub type ServiceName = String;

/// Loaded, validated catalog.
#[derive(Debug, Clone)]
pub struct Catalog {
    /// service → endpoint name → def.
    pub services: BTreeMap<ServiceName, BTreeMap<String, EndpointDef>>,
}

impl Catalog {
    pub fn load_embedded() -> CliResult<Self> {
        let mut services: BTreeMap<String, BTreeMap<String, EndpointDef>> = BTreeMap::new();

        for service_dir in CATALOG_DIR.dirs() {
            let svc_name = service_dir
                .path()
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .replace('_', "-");
            if svc_name.is_empty() {
                continue;
            }
            let entry = services.entry(svc_name.clone()).or_default();
            for file in service_dir.files() {
                let path = file.path();
                let ext = path
                    .extension()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default();
                if ext != "yaml" && ext != "yml" {
                    continue;
                }
                let raw = file.contents_utf8().ok_or_else(|| {
                    CliError::config(format!("manifest {} is not valid UTF-8", path.display()))
                })?;
                let def: EndpointDef = serde_yaml::from_str(raw).map_err(|e| {
                    CliError::config(format!("manifest {}: parse error: {e}", path.display()))
                })?;
                validate_def(&svc_name, &def, &path.display().to_string())?;
                if entry.contains_key(&def.name) {
                    return Err(CliError::config(format!(
                        "service `{svc_name}`: duplicate endpoint name `{}` (file: {})",
                        def.name,
                        path.display()
                    )));
                }
                entry.insert(def.name.clone(), def);
            }
        }

        Ok(Catalog { services })
    }

    pub fn list_services(&self) -> Vec<&str> {
        self.services.keys().map(|s| s.as_str()).collect()
    }

    pub fn get(&self, service: &str, name: &str) -> Option<&EndpointDef> {
        self.services.get(service).and_then(|m| m.get(name))
    }

    pub fn endpoints_for<'a>(
        &'a self,
        service: &str,
    ) -> Option<impl Iterator<Item = &'a EndpointDef>> {
        self.services.get(service).map(|m| m.values())
    }
}

const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "HEAD", "OPTIONS"];

/// Public wrapper for catalog-load validation rules. Exposed for tests that drive
/// the validator on hand-rolled manifests.
pub fn validate_endpoint(def: &EndpointDef) -> CliResult<()> {
    validate_def("test", def, "<inline>")
}

fn validate_def(svc: &str, def: &EndpointDef, file: &str) -> CliResult<()> {
    if def.name.is_empty() {
        return Err(CliError::config(format!("{file}: empty `name` field")));
    }
    let method_upper = def.method.to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&method_upper.as_str()) {
        return Err(CliError::config(format!(
            "{file}: invalid method `{}` (allowed: {})",
            def.method,
            ALLOWED_METHODS.join(", ")
        )));
    }
    if !def.path.starts_with('/') {
        return Err(CliError::config(format!(
            "{file}: `path` must start with `/` (got `{}`)",
            def.path
        )));
    }
    let bodyless = matches!(method_upper.as_str(), "GET" | "HEAD");
    if bodyless && def.body_template.is_some() {
        return Err(CliError::config(format!(
            "{file}: {} endpoint must not declare `body_template`",
            method_upper
        )));
    }

    // Names declared.
    let mut declared: BTreeMap<String, VarKind> = BTreeMap::new();
    for r in &def.required_vars {
        if declared.insert(r.name.clone(), r.kind).is_some() {
            return Err(CliError::config(format!(
                "{file}: duplicate var `{}` in required_vars",
                r.name
            )));
        }
    }
    for o in &def.optional_vars {
        if declared.insert(o.name.clone(), o.kind).is_some() {
            return Err(CliError::config(format!(
                "{file}: duplicate var `{}` in optional_vars (also in required_vars?)",
                o.name
            )));
        }
    }

    // Path placeholders must be declared as string-typed vars.
    for placeholder in extract_path_placeholders(&def.path) {
        let kind = declared.get(&placeholder).copied().ok_or_else(|| {
            CliError::config(format!(
                "{file}: path references undeclared var `{placeholder}` (declared: {})",
                declared.keys().cloned().collect::<Vec<_>>().join(", ")
            ))
        })?;
        if kind != VarKind::String {
            return Err(CliError::config(format!(
                "{file}: path var `{placeholder}` must be kind=string (got {:?})",
                kind
            )));
        }
    }

    if let Some(tmpl) = &def.body_template {
        // Pre-parse with Tera so the load-time scanner shares Tera's grammar.
        // Anything the runtime would reject (unterminated `{{`, malformed filter
        // expressions, etc.) surfaces here as a Config error.
        let mut probe = tera::Tera::default();
        probe.autoescape_on(vec![]);
        if let Err(e) = probe.add_raw_template("body", tmpl) {
            return Err(CliError::config(format!(
                "{file}: body_template parse error: {e}"
            )));
        }

        // Find every `{{ ... }}` group; for each, parse the var name and required filter.
        let refs = extract_template_refs(tmpl);
        for r in refs {
            let kind = declared.get(&r.name).copied().ok_or_else(|| {
                CliError::config(format!(
                    "{file}: body_template references undeclared var `{}` (declared: {})",
                    r.name,
                    declared.keys().cloned().collect::<Vec<_>>().join(", ")
                ))
            })?;
            match kind {
                VarKind::String => {
                    if !r.filters.iter().any(|f| f == "json_encode") {
                        return Err(CliError::config(format!(
                            "{file}: body_template references string-typed var `{}` without `| json_encode` (use `{{{{ {} | json_encode }}}}`)",
                            r.name, r.name
                        )));
                    }
                }
                VarKind::Json => {
                    // `kind: json` vars MUST be referenced with either `| json_encode`
                    // (re-encodes the JSON value as a JSON string) OR `| safe` (passes
                    // pre-validated JSON through without escaping). A raw reference is
                    // a manifest authoring bug.
                    let has_filter = r.filters.iter().any(|f| f == "json_encode" || f == "safe");
                    if !has_filter {
                        return Err(CliError::config(format!(
                            "{file}: body_template references json-typed var `{}` without `| json_encode` or `| safe` (use `{{{{ {} | safe }}}}` for pre-validated JSON)",
                            r.name, r.name
                        )));
                    }
                }
            }
        }
    }

    // Bind svc to silence unused param when the function grows.
    let _ = svc;
    Ok(())
}

/// One `{{ ... }}` reference parsed out of a Tera template.
#[derive(Debug, Clone)]
struct TemplateRef {
    name: String,
    filters: Vec<String>,
}

fn extract_path_placeholders(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j > start && j < bytes.len() {
                out.push(path[start..j].to_string());
                i = j + 1;
                continue;
            }
        }
        i += 1;
    }
    out
}

fn extract_template_refs(template: &str) -> Vec<TemplateRef> {
    let mut out = Vec::new();
    let bytes = template.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // Find closing `}}`.
            let start = i + 2;
            let mut j = start;
            while j + 1 < bytes.len() && !(bytes[j] == b'}' && bytes[j + 1] == b'}') {
                j += 1;
            }
            if j + 1 >= bytes.len() {
                break;
            }
            let inner = &template[start..j];
            let parts: Vec<&str> = inner.split('|').map(|s| s.trim()).collect();
            if let Some(first) = parts.first() {
                // Strip whitespace and a possible `(`-paren expression.
                // We only accept simple identifiers.
                let name = first.split_whitespace().next().unwrap_or("").to_string();
                if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    let filters = parts[1..]
                        .iter()
                        .map(|f| {
                            f.split(|c: char| c == '(' || c.is_whitespace())
                                .next()
                                .unwrap_or("")
                                .to_string()
                        })
                        .collect();
                    out.push(TemplateRef { name, filters });
                }
            }
            i = j + 2;
        } else {
            i += 1;
        }
    }
    out
}

/// JSON shape used by `describe` / `list-endpoints` (schema_version "1").
#[derive(Debug, Serialize)]
pub struct DescribeOutput<'a> {
    pub schema_version: &'static str,
    pub service: &'a str,
    pub name: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub description: &'a str,
    pub auth: &'a Auth,
    pub required_vars: &'a [RequiredVar],
    pub optional_vars: &'a [OptionalVar],
    pub body_template_preview: Option<&'a str>,
    pub target_url_pattern: String,
}

#[derive(Debug, Serialize)]
pub struct ListEntry<'a> {
    pub service: &'a str,
    pub name: &'a str,
    pub method: &'a str,
    pub path: &'a str,
    pub description: &'a str,
    pub auth: &'a Auth,
    pub required_vars: &'a [RequiredVar],
}
