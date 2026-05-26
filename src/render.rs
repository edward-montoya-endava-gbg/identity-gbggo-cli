//! Tera body rendering with JSON-safe filters and var resolution.
//!
//! - `--var key=value` > process env `GGO_VAR_<UPPER_KEY>` > `optional_vars[].default`.
//! - Required-var enforcement before any HTTP/auth.
//! - `kind: json` vars validated via `serde_json::from_str` before substitution.
//! - String-typed vars use Tera's built-in `json_encode` filter which emits the quotes.

use crate::endpoints::{EndpointView, VarKind};
use crate::error::{CliError, CliResult};
use percent_encoding::{AsciiSet, CONTROLS};
use std::collections::BTreeMap;
use tera::{Context, Tera, Value};

/// Characters that must be percent-encoded inside a URL path segment.
const PATH_ENCODE_SET: &AsciiSet = &CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'<')
    .add(b'>')
    .add(b'\\')
    .add(b'^')
    .add(b'`')
    .add(b'{')
    .add(b'}')
    .add(b'|')
    .add(b'#')
    .add(b'?')
    .add(b'%')
    .add(b'/')
    .add(b'&')
    .add(b';')
    .add(b'=')
    .add(b'+')
    .add(b'@')
    .add(b':')
    .add(b'\r')
    .add(b'\n');

/// Result of variable resolution.
#[derive(Debug)]
pub struct ResolvedVars {
    /// All resolved values, kind-tagged.
    pub values: BTreeMap<String, ResolvedValue>,
}

#[derive(Debug)]
pub enum ResolvedValue {
    Str(String),
    Json(Value),
}

/// Resolve and validate every variable referenced by an endpoint view.
pub fn resolve_vars(
    view: &EndpointView<'_>,
    cli_vars: &BTreeMap<String, String>,
) -> CliResult<ResolvedVars> {
    let mut out: BTreeMap<String, ResolvedValue> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();

    for r in view.required_vars {
        match resolve_one(&r.name, r.kind, cli_vars, None)? {
            Some(v) => {
                out.insert(r.name.clone(), v);
            }
            None => missing.push(r.name.clone()),
        }
    }
    if !missing.is_empty() {
        return Err(CliError::usage(format!(
            "missing required vars: {} (supply --var KEY=VALUE or GGO_VAR_{}=...)",
            missing.join(", "),
            missing[0].to_ascii_uppercase()
        )));
    }
    for o in view.optional_vars {
        if let Some(v) = resolve_one(&o.name, o.kind, cli_vars, o.default.as_deref())? {
            out.insert(o.name.clone(), v);
        }
    }

    Ok(ResolvedVars { values: out })
}

fn resolve_one(
    name: &str,
    kind: VarKind,
    cli_vars: &BTreeMap<String, String>,
    default: Option<&str>,
) -> CliResult<Option<ResolvedValue>> {
    let raw = cli_vars
        .get(name)
        .cloned()
        .or_else(|| std::env::var(format!("GGO_VAR_{}", name.to_ascii_uppercase())).ok())
        .or_else(|| default.map(|s| s.to_string()));

    let Some(raw) = raw else {
        return Ok(None);
    };

    match kind {
        VarKind::String => Ok(Some(ResolvedValue::Str(raw))),
        VarKind::Json => {
            let v: Value = serde_json::from_str(&raw).map_err(|e| {
                CliError::usage(format!(
                    "var `{name}` declared kind=json but value is not valid JSON: {e} (value: {})",
                    truncate(&raw, 80)
                ))
            })?;
            Ok(Some(ResolvedValue::Json(v)))
        }
    }
}

/// Render the body template using Tera, with `json_encode` available natively.
pub fn render_body(view: &EndpointView<'_>, vars: &ResolvedVars) -> CliResult<Option<String>> {
    let Some(tmpl_src) = view.body_template else {
        return Ok(None);
    };

    let mut tera = Tera::default();
    tera.autoescape_on(vec![]); // bodies are JSON, not HTML
    tera.add_raw_template("body", tmpl_src)
        .map_err(|e| CliError::config(format!("body_template compile error: {e}")))?;

    let mut ctx = Context::new();
    for (k, v) in &vars.values {
        match v {
            ResolvedValue::Str(s) => ctx.insert(k, s),
            ResolvedValue::Json(j) => ctx.insert(k, j),
        }
    }

    let rendered = tera
        .render("body", &ctx)
        .map_err(|e| CliError::config(format!("body_template render error: {e}")))?;

    // Validate the rendered output is itself parseable JSON.
    if let Err(e) = serde_json::from_str::<Value>(&rendered) {
        return Err(CliError::config(format!(
            "rendered body is not valid JSON: {e}; rendered: {}",
            truncate(&rendered, 200)
        )));
    }

    Ok(Some(rendered))
}

/// Substitute `{name}` path placeholders with the corresponding string-typed var value.
pub fn substitute_path(path: &str, vars: &ResolvedVars) -> CliResult<String> {
    let mut out = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'{' {
            let start = i + 1;
            let mut j = start;
            while j < bytes.len() && bytes[j] != b'}' {
                j += 1;
            }
            if j >= bytes.len() {
                return Err(CliError::config(format!(
                    "unterminated `{{` in endpoint path `{path}`"
                )));
            }
            let name = &path[start..j];
            let v = vars.values.get(name).ok_or_else(|| {
                CliError::usage(format!("path references var `{name}` but it is not set"))
            })?;
            match v {
                ResolvedValue::Str(s) => {
                    let encoded =
                        percent_encoding::utf8_percent_encode(s, PATH_ENCODE_SET).to_string();
                    out.push_str(&encoded);
                }
                ResolvedValue::Json(_) => {
                    return Err(CliError::usage(format!(
                        "path var `{name}` must be kind=string, not kind=json"
                    )));
                }
            }
            i = j + 1;
        } else {
            out.push(bytes[i] as char);
            i += 1;
        }
    }
    Ok(out)
}

fn truncate(s: &str, n: usize) -> String {
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i >= n {
            out.push('…');
            break;
        }
        out.push(c);
    }
    out
}
