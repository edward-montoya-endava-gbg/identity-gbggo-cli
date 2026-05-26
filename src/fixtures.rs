//! Test-data fixture catalog.
//!
//! Fixtures are YAML (preferred) or JSON files that compose Captain endpoint
//! request bodies — primarily the `context` object — without forcing operators
//! to hand-craft JSON. Built-in fixtures ship inside the binary via
//! `include_dir!`; users may also point `--include` at a filesystem path.
//!
//! ## File format
//!
//! Fixtures are parsed as YAML/JSON and walked recursively. Any string value
//! exactly matching the sentinel `@file:<relative-path>` is replaced by the
//! UTF-8 contents of the embedded file at `src/fixtures/<relative-path>`. This
//! is how biometrics / documents fixtures inline base64-encoded image bytes
//! without bloating the YAML.

use crate::error::{CliError, CliResult};
use include_dir::{include_dir, Dir};
use serde_json::Value;
use std::path::Path;

/// Embedded fixtures directory — baked into the binary at compile time.
static FIXTURES_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/src/fixtures");

/// Sentinel prefix for inlined file references inside a fixture.
const FILE_SENTINEL: &str = "@file:";

/// Resolve a fixture by `(category, name)`.
///
/// Lookup order:
/// 1. Built-in: `src/fixtures/<category>/<name>.yaml` (or `.yml` / `.json`).
/// 2. Filesystem fallback when `name` contains `/` or `.`.
/// 3. Usage error listing the built-in fixtures for the category.
pub fn resolve(category: &str, name: &str) -> CliResult<Value> {
    let raw = load_raw(category, name)?;
    let mut value = parse_raw(&raw.contents, &raw.source, raw.ext.as_deref())?;
    inline_file_refs(&mut value)?;
    Ok(value)
}

/// List every built-in fixture as `(category, name)` pairs, sorted.
///
/// When `category` is `Some`, only fixtures under that category are returned;
/// `None` returns the full catalog.
pub fn list(category: Option<&str>) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for dir in FIXTURES_DIR.dirs() {
        let cat = match dir.path().file_name().and_then(|s| s.to_str()) {
            Some(s) => s.to_string(),
            None => continue,
        };
        // The `images/` directory holds raw inlined assets, not fixtures themselves.
        if cat == "images" {
            continue;
        }
        if let Some(filter) = category {
            if filter != cat {
                continue;
            }
        }
        for file in dir.files() {
            let path = file.path();
            let ext = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if !is_supported_ext(ext) {
                continue;
            }
            let stem = match path.file_stem().and_then(|s| s.to_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };
            out.push((cat.clone(), stem));
        }
    }
    out.sort();
    out
}

/// Return the list of built-in fixture names for a category (alphabetical).
pub fn list_names_for_category(category: &str) -> Vec<String> {
    list(Some(category)).into_iter().map(|(_, n)| n).collect()
}

struct LoadedFixture {
    contents: String,
    source: String,
    ext: Option<String>,
}

fn load_raw(category: &str, name: &str) -> CliResult<LoadedFixture> {
    // Try built-in with .yaml / .yml / .json (in that order).
    for ext in ["yaml", "yml", "json"] {
        let rel = format!("{category}/{name}.{ext}");
        if let Some(file) = FIXTURES_DIR.get_file(&rel) {
            let contents = file
                .contents_utf8()
                .ok_or_else(|| {
                    CliError::config(format!("built-in fixture `{rel}` is not valid UTF-8"))
                })?
                .to_string();
            return Ok(LoadedFixture {
                contents,
                source: format!("<built-in:{rel}>"),
                ext: Some(ext.to_string()),
            });
        }
    }

    // Filesystem fallback when the name looks like a path.
    let looks_like_path = name.contains('/') || name.contains('.');
    if looks_like_path {
        let path = Path::new(name);
        let contents = std::fs::read_to_string(path).map_err(|e| {
            CliError::usage(format!(
                "fixture `{}/{}`: file not found: {} ({})",
                category,
                name,
                path.display(),
                e
            ))
        })?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string());
        return Ok(LoadedFixture {
            contents,
            source: path.display().to_string(),
            ext,
        });
    }

    let available = list_names_for_category(category);
    let avail_str = if available.is_empty() {
        format!("none (no built-in fixtures registered for `{category}`)")
    } else {
        available.join(", ")
    };
    Err(CliError::usage(format!(
        "fixture `{category}/{name}` not found; built-in fixtures: {avail_str} (use `goctl list-fixtures --category {category}` to see them) or pass a path."
    )))
}

fn parse_raw(contents: &str, source: &str, ext: Option<&str>) -> CliResult<Value> {
    let is_json = matches!(ext, Some("json"));
    if is_json {
        serde_json::from_str(contents)
            .map_err(|e| CliError::usage(format!("fixture `{source}`: JSON parse error: {e}")))
    } else {
        // Default to YAML for `.yaml`, `.yml`, or unknown — YAML is a
        // superset of JSON for our purposes.
        serde_yaml::from_str(contents)
            .map_err(|e| CliError::usage(format!("fixture `{source}`: YAML parse error: {e}")))
    }
}

fn is_supported_ext(ext: &str) -> bool {
    matches!(ext, "yaml" | "yml" | "json")
}

/// Walk `value` recursively, replacing every `String("@file:<path>")` with the
/// embedded file's contents. Errors when the referenced file is missing.
fn inline_file_refs(value: &mut Value) -> CliResult<()> {
    match value {
        Value::String(s) => {
            if let Some(rel) = s.strip_prefix(FILE_SENTINEL) {
                let resolved = read_embedded_text(rel)?;
                *s = resolved;
            }
            Ok(())
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                inline_file_refs(item)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            for (_k, v) in map.iter_mut() {
                inline_file_refs(v)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn read_embedded_text(rel: &str) -> CliResult<String> {
    let file = FIXTURES_DIR.get_file(rel).ok_or_else(|| {
        CliError::config(format!("fixture references missing embedded file: {rel}"))
    })?;
    let text = file.contents_utf8().ok_or_else(|| {
        CliError::config(format!(
            "fixture references embedded file `{rel}` but it is not valid UTF-8"
        ))
    })?;
    // Trim a single trailing newline so inlined base64 doesn't carry stray
    // whitespace into the rendered JSON body. We deliberately preserve
    // interior whitespace.
    Ok(text.trim_end_matches('\n').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identity_v1_loads_with_no_file_refs() {
        let v = resolve("identity", "testdata-v1").expect("loads");
        assert_eq!(v["firstName"], "Sean");
        assert_eq!(v["address"]["country"], "GB");
    }

    #[test]
    fn biometrics_default_inlines_image_files() {
        let v = resolve("biometrics", "default").expect("loads");
        let selfie = v["selfieImage"].as_str().expect("string");
        let anchor = v["anchorImage"].as_str().expect("string");
        assert!(!selfie.is_empty());
        assert!(!anchor.is_empty());
        assert!(
            !selfie.starts_with(FILE_SENTINEL),
            "selfie not inlined: {selfie}"
        );
        assert!(
            !anchor.starts_with(FILE_SENTINEL),
            "anchor not inlined: {anchor}"
        );
    }

    #[test]
    fn list_excludes_images_dir() {
        let all = list(None);
        assert!(!all.is_empty());
        assert!(
            all.iter().all(|(c, _)| c != "images"),
            "images dir leaked into catalog"
        );
    }

    #[test]
    fn unknown_fixture_returns_usage() {
        let err = resolve("identity", "no-such-fixture").unwrap_err();
        assert_eq!(err.kind.code(), 2);
        assert!(err.to_string().contains("not found"), "got: {err}");
    }
}
