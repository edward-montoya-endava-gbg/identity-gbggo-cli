//! JSON / table response formatting and dry-run printing.

use crate::error::{CliError, CliResult};
use comfy_table::{ContentArrangement, Table};
use serde_json::Value;

/// Print a dry-run summary to stdout. `Authorization` value is always redacted as `Bearer ***`.
pub fn print_dry_run(method: &str, url: &str, has_auth: bool, body: Option<&str>) {
    println!("{method} {url}");
    if has_auth {
        println!("Authorization: Bearer ***");
    }
    if let Some(b) = body {
        println!("\n{b}");
    }
}

/// Print a successful response body.
/// - JSON pass-through by default.
/// - With `--table`, render as a comfy-table if the response is a homogeneous array of objects;
///   otherwise fall back to JSON pass-through with a stderr note (no silent row drops).
pub fn print_response(body: &str, want_table: bool) -> CliResult<()> {
    if !want_table {
        println!("{body}");
        return Ok(());
    }

    let parsed: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(_) => {
            eprintln!("note: --table requested but response is not JSON; printing as text");
            println!("{body}");
            return Ok(());
        }
    };

    let Value::Array(items) = parsed else {
        eprintln!("note: --table requested but response is not a JSON array; printing JSON");
        println!("{body}");
        return Ok(());
    };

    // First-element shape decides; if any element diverges, fall back.
    let Some(first) = items.first() else {
        println!("[]");
        return Ok(());
    };

    let first_obj = match first.as_object() {
        Some(o) => o,
        None => {
            eprintln!(
                "note: --table requested but first array element is not an object; printing JSON"
            );
            println!("{body}");
            return Ok(());
        }
    };

    let headers: Vec<String> = first_obj.keys().cloned().collect();

    // Verify all elements are objects with the same key set (no silent row drops).
    for (i, item) in items.iter().enumerate() {
        match item.as_object() {
            None => {
                eprintln!(
                    "note: --table: element {i} is not an object; falling back to JSON output"
                );
                println!("{body}");
                return Ok(());
            }
            Some(o) => {
                if o.keys().count() != headers.len() || !headers.iter().all(|h| o.contains_key(h)) {
                    eprintln!(
                        "note: --table: heterogeneous element keys at index {i}; falling back to JSON output"
                    );
                    println!("{body}");
                    return Ok(());
                }
            }
        }
    }

    let mut table = Table::new();
    table.set_content_arrangement(ContentArrangement::Dynamic);
    table.set_header(&headers);

    for item in &items {
        let obj = item.as_object().unwrap();
        let row: Vec<String> = headers
            .iter()
            .map(|h| value_as_cell(obj.get(h).unwrap_or(&Value::Null)))
            .collect();
        table.add_row(row);
    }

    println!("{table}");
    Ok(())
}

fn value_as_cell(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// Decode upstream response body using lossy UTF-8 (never drop bytes silently).
pub fn body_lossy(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

/// Truncate a body for inclusion in an error message.
pub fn truncate(s: &str, n: usize) -> String {
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

/// Build an upstream error message including status, body snippet, and `X-Request-ID`.
pub fn upstream_error_message(
    method: &str,
    url: &str,
    status: u16,
    request_id: Option<&str>,
    body: &str,
) -> String {
    let rid = request_id.unwrap_or("<none>");
    let snippet = truncate(body, 200);
    format!("{method} {url} → {status} (X-Request-ID: {rid}) body: {snippet}")
}

/// Sentinel used to coerce JSON output even when stdout is a TTY.
pub fn must_print_json(v: &Value) -> CliResult<()> {
    let s = serde_json::to_string(v)
        .map_err(|e| CliError::config(format!("internal: cannot serialize JSON: {e}")))?;
    println!("{s}");
    Ok(())
}
