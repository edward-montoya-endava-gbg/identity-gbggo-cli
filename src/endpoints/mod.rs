//! Endpoint catalog: YAML manifests embedded at compile time via `include_dir!`.
//!
//! Validates manifests at startup:
//! - method in {GET, POST, PUT, PATCH, DELETE, HEAD, OPTIONS}
//! - path starts with `/`
//! - GET (and HEAD) endpoints have no `body_template`
//! - no intra-service name collisions
//! - every `{{ var }}` in `body_template` is declared in required_vars ∪ optional_vars
//! - every string-typed var reference in body_template uses `| json_encode`
//!
//! Two manifest shapes are supported:
//! - **Flat** (non-versioned services): the original schema with top-level
//!   `method`, `path`, `required_vars`, etc.
//! - **Versioned** (e.g. `captain`): a `supported_versions: [...]` list plus a
//!   `versions: { v1: {...}, v2: {...} }` map. Each version carries its own
//!   `method`, `path`, vars, body template, and per-version `disabled` /
//!   `deprecated` flags.

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

/// A single version of a versioned endpoint. Same shape as a flat endpoint
/// minus name/description/auth (which live on the parent `EndpointDef`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedEndpoint {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub required_vars: Vec<RequiredVar>,
    #[serde(default)]
    pub optional_vars: Vec<OptionalVar>,
    #[serde(default)]
    pub body_template: Option<String>,
    /// Per-version disabled gate. Same semantics as the flat `disabled` field
    /// but scoped to a single version.
    #[serde(default)]
    pub disabled: bool,
    /// One-line reason surfaced in the disabled-gate error message.
    #[serde(default)]
    pub disabled_reason: Option<String>,
    /// When set, emit a one-shot `tracing::warn!` before execution and
    /// continue. Use to flag legacy versions slated for retirement.
    #[serde(default)]
    pub deprecated: Option<String>,
}

/// The flat (non-versioned) endpoint shape — the original manifest schema.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FlatEndpoint {
    pub method: String,
    pub path: String,
    #[serde(default)]
    pub required_vars: Vec<RequiredVar>,
    #[serde(default)]
    pub optional_vars: Vec<OptionalVar>,
    #[serde(default)]
    pub body_template: Option<String>,
    /// When true, the endpoint refuses to run at the very top of `exec::run`,
    /// before any auth/var resolution or `--dry-run` short-circuit.
    #[serde(default)]
    pub disabled: bool,
    /// One-line reason surfaced in the disabled-gate error message.
    #[serde(default)]
    pub disabled_reason: Option<String>,
}

/// Mutually exclusive payload — either a flat endpoint or a versioned one.
///
/// We use `untagged` so the YAML loader picks the variant by presence of
/// `supported_versions` / `versions` vs flat `method`/`path`. The variant order
/// matters: `Versioned` is tried first so a manifest with both shapes (a bug)
/// fails on `Flat`'s `deny_unknown_fields`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum EndpointBody {
    /// Tried first: any manifest declaring `supported_versions` / `versions`
    /// matches `VersionedBody` (newtype with `deny_unknown_fields`). Flat
    /// manifests (no such keys) fall through to `Flat`. Authoring typos at
    /// either layer are rejected at parse time because both wrapped structs
    /// carry `deny_unknown_fields`.
    Versioned(VersionedBody),
    Flat(FlatEndpoint),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VersionedBody {
    pub supported_versions: Vec<String>,
    pub versions: BTreeMap<String, VersionedEndpoint>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointDef {
    pub name: String,
    pub description: String,
    pub auth: Auth,
    // `deny_unknown_fields` cannot be combined with `#[serde(flatten)]` of an
    // untagged enum (serde collects unknown fields into the flattened payload
    // and the untagged matcher decides which variant fits). The inner
    // `FlatEndpoint` / `VersionedEndpoint` structs carry `deny_unknown_fields`
    // so authoring typos there still error at parse time.
    #[serde(flatten)]
    pub body: EndpointBody,
}

impl EndpointDef {
    /// Convenience: list the supported versions for a versioned endpoint, or
    /// `None` for a flat endpoint.
    pub fn supported_versions(&self) -> Option<&[String]> {
        match &self.body {
            EndpointBody::Versioned(v) => Some(v.supported_versions.as_slice()),
            EndpointBody::Flat(_) => None,
        }
    }

    /// Borrow the flat endpoint (or the chosen version's payload) for a given
    /// resolved version. Returns `None` if the version is missing from a
    /// versioned endpoint, and ignores `version` for flat endpoints.
    ///
    /// The returned view's `version` field borrows from the matched key inside
    /// `self.versions`, so its lifetime is tied to `&self`.
    pub fn resolve_version(&self, version: Option<&str>) -> Option<EndpointView<'_>> {
        match &self.body {
            EndpointBody::Flat(f) => Some(EndpointView {
                method: &f.method,
                path: &f.path,
                required_vars: &f.required_vars,
                optional_vars: &f.optional_vars,
                body_template: f.body_template.as_deref(),
                disabled: f.disabled,
                disabled_reason: f.disabled_reason.as_deref(),
                deprecated: None,
                version: None,
            }),
            EndpointBody::Versioned(vb) => {
                let v = version?;
                let (key, ve) = vb.versions.get_key_value(v)?;
                Some(EndpointView {
                    method: &ve.method,
                    path: &ve.path,
                    required_vars: &ve.required_vars,
                    optional_vars: &ve.optional_vars,
                    body_template: ve.body_template.as_deref(),
                    disabled: ve.disabled,
                    disabled_reason: ve.disabled_reason.as_deref(),
                    deprecated: ve.deprecated.as_deref(),
                    version: Some(key.as_str()),
                })
            }
        }
    }

    /// True when every supported version (or the flat body) is disabled.
    pub fn is_fully_disabled(&self) -> bool {
        match &self.body {
            EndpointBody::Flat(f) => f.disabled,
            EndpointBody::Versioned(vb) => {
                !vb.versions.is_empty() && vb.versions.values().all(|v| v.disabled)
            }
        }
    }
}

/// Borrowed view of a single (endpoint, version) combination — the common
/// shape that auth / render / exec all reason over.
#[derive(Debug, Clone)]
pub struct EndpointView<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub required_vars: &'a [RequiredVar],
    pub optional_vars: &'a [OptionalVar],
    pub body_template: Option<&'a str>,
    pub disabled: bool,
    pub disabled_reason: Option<&'a str>,
    pub deprecated: Option<&'a str>,
    /// Version key (e.g. "v1") for a versioned endpoint, or `None` for flat.
    pub version: Option<&'a str>,
}

impl<'a> EndpointView<'a> {
    pub fn var_kind(&self, name: &str) -> Option<VarKind> {
        for r in self.required_vars {
            if r.name == name {
                return Some(r.kind);
            }
        }
        for o in self.optional_vars {
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
    match &def.body {
        EndpointBody::Flat(flat) => validate_flat(file, flat)?,
        EndpointBody::Versioned(vb) => {
            if vb.supported_versions.is_empty() {
                return Err(CliError::config(format!(
                    "{file}: `supported_versions` must list at least one version"
                )));
            }
            for sv in &vb.supported_versions {
                if !vb.versions.contains_key(sv) {
                    return Err(CliError::config(format!(
                        "{file}: supported_versions includes `{sv}` but no `versions.{sv}` block"
                    )));
                }
            }
            for k in vb.versions.keys() {
                if !vb.supported_versions.contains(k) {
                    return Err(CliError::config(format!(
                        "{file}: `versions.{k}` defined but `{k}` is not in supported_versions"
                    )));
                }
            }
            for (v, ve) in &vb.versions {
                let where_ = format!("{file} (version {v})");
                validate_versioned(&where_, ve)?;
            }
        }
    }
    let _ = svc;
    Ok(())
}

fn validate_flat(file: &str, def: &FlatEndpoint) -> CliResult<()> {
    validate_request(
        file,
        &def.method,
        &def.path,
        &def.required_vars,
        &def.optional_vars,
        def.body_template.as_deref(),
    )
}

fn validate_versioned(file: &str, def: &VersionedEndpoint) -> CliResult<()> {
    validate_request(
        file,
        &def.method,
        &def.path,
        &def.required_vars,
        &def.optional_vars,
        def.body_template.as_deref(),
    )
}

fn validate_request(
    file: &str,
    method: &str,
    path: &str,
    required_vars: &[RequiredVar],
    optional_vars: &[OptionalVar],
    body_template: Option<&str>,
) -> CliResult<()> {
    let method_upper = method.to_ascii_uppercase();
    if !ALLOWED_METHODS.contains(&method_upper.as_str()) {
        return Err(CliError::config(format!(
            "{file}: invalid method `{}` (allowed: {})",
            method,
            ALLOWED_METHODS.join(", ")
        )));
    }
    if !path.starts_with('/') {
        return Err(CliError::config(format!(
            "{file}: `path` must start with `/` (got `{}`)",
            path
        )));
    }
    let bodyless = matches!(method_upper.as_str(), "GET" | "HEAD");
    if bodyless && body_template.is_some() {
        return Err(CliError::config(format!(
            "{file}: {} endpoint must not declare `body_template`",
            method_upper
        )));
    }

    // Names declared.
    let mut declared: BTreeMap<String, VarKind> = BTreeMap::new();
    for r in required_vars {
        if declared.insert(r.name.clone(), r.kind).is_some() {
            return Err(CliError::config(format!(
                "{file}: duplicate var `{}` in required_vars",
                r.name
            )));
        }
    }
    for o in optional_vars {
        if declared.insert(o.name.clone(), o.kind).is_some() {
            return Err(CliError::config(format!(
                "{file}: duplicate var `{}` in optional_vars (also in required_vars?)",
                o.name
            )));
        }
    }

    // Path placeholders must be declared as string-typed vars.
    for placeholder in extract_path_placeholders(path) {
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

    if let Some(tmpl) = body_template {
        // Pre-parse with Tera so the load-time scanner shares Tera's grammar.
        let mut probe = tera::Tera::default();
        probe.autoescape_on(vec![]);
        if let Err(e) = probe.add_raw_template("body", tmpl) {
            return Err(CliError::config(format!(
                "{file}: body_template parse error: {e}"
            )));
        }

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

/// Per-version slice surfaced in `describe` JSON for a versioned endpoint.
#[derive(Debug, Serialize)]
pub struct DescribeVersion<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub required_vars: &'a [RequiredVar],
    pub optional_vars: &'a [OptionalVar],
    pub body_template: Option<&'a str>,
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<&'a str>,
    pub deprecated: Option<&'a str>,
    pub target_url_pattern: String,
}

/// JSON shape used by `describe` (schema_version "1").
///
/// Schema is additive: flat endpoints emit `method` / `path` / etc as before;
/// versioned endpoints emit `supported_versions` + `versions:` map.
#[derive(Debug, Serialize)]
#[serde(untagged)]
pub enum DescribeOutput<'a> {
    Flat {
        schema_version: &'static str,
        service: &'a str,
        name: &'a str,
        method: &'a str,
        path: &'a str,
        description: &'a str,
        auth: &'a Auth,
        required_vars: &'a [RequiredVar],
        optional_vars: &'a [OptionalVar],
        body_template_preview: Option<&'a str>,
        target_url_pattern: String,
        disabled: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        disabled_reason: Option<&'a str>,
    },
    Versioned {
        schema_version: &'static str,
        service: &'a str,
        name: &'a str,
        description: &'a str,
        auth: &'a Auth,
        supported_versions: &'a [String],
        versions: BTreeMap<String, DescribeVersion<'a>>,
    },
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
    pub disabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disabled_reason: Option<&'a str>,
    /// Present only for versioned endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supported_versions: Option<&'a [String]>,
}
