# goctl — LLM usage guide

This file is a copy-pasteable system-prompt fragment for LLM agents using
`goctl`. The JSON contracts below are versioned with `schema_version: "1"`.

## Command grammar

```
goctl --env <ENV> [--region <REGION>] <SERVICE> <COMMAND>
      [--var KEY=VALUE ...]
      [--token <BEARER_JWT> | --token-url <URL>]
      [--config <PATH>]
      [--dry-run] [--confirm]
      [--json] [--json-errors] [--table]

goctl list-endpoints [--service <SERVICE>] [--json]
goctl describe <SERVICE> <COMMAND> [--json]
```

- `ENV` ∈ `dev | demo | prod`.
- `REGION` ∈ `au | eu | us` (case-insensitive; optional when an env defines
  only one region).
- `SERVICE` ∈ `captain-v1 | captain-v2 | designer | userview`.
- `COMMAND` is one of the manifest names. Enumerate with
  `goctl list-endpoints --service <SERVICE> --json`.

## Variable substitution

Template vars are supplied with repeated `--var KEY=VALUE`. Precedence:
`--var` > `$GGO_VAR_<UPPER_KEY>` > the manifest's `optional_vars[].default`.

Each var declares a `kind`:

- `kind: string` — passed through Tera's `json_encode` filter; arbitrary
  values with quotes, backslashes, or newlines are safely escaped.
- `kind: json` — pre-validated with `serde_json::from_str`; an invalid value
  exits 2 *before* any HTTP or auth step.

## Auth

- `--token <JWT>` (or `GGO_BEARER_TOKEN`): bypasses OAuth; the bearer is
  reused until its `exp` claim lapses (30s skew tolerance).
- Designer prod and UserView prod are bearer-only — without a bearer the
  CLI exits 2.
- Endpoints whose manifest declares `auth: none` skip the bearer-only gate
  and reach upstream with no `Authorization` header.

## Output contracts

`goctl describe <SERVICE> <COMMAND> --json` →

```json
{
  "schema_version": "1",
  "service": "captain-v2",
  "name": "journey-start",
  "method": "POST",
  "path": "/v2/captain/journey/start",
  "description": "...",
  "auth": "customer",
  "required_vars": [
    {"name": "resourceId", "kind": "string", "description": "...", "example": "..."}
  ],
  "optional_vars": [...],
  "body_template_preview": "{ ... }",
  "target_url_pattern": "<base_url>/v2/captain/journey/start"
}
```

`goctl list-endpoints --service designer --json` → JSON array of
`{service, name, method, path, description, auth, required_vars}` entries.

`goctl --json-errors ...` → on error, a single-line JSON object on stderr:

```json
{"schema_version":"1","exit_code":2,"kind":"Usage","message":"...","context":"..."}
```

## Exit codes

| Code | `kind` |
|---|---|
| 0 | success |
| 1 | `Config` |
| 2 | `Usage` |
| 3 | `Auth` |
| 4 | `UpstreamClient` |
| 5 | `UpstreamServer` or `Network` |
