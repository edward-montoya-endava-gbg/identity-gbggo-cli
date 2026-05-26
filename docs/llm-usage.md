# goctl — LLM usage guide

This file is a copy-pasteable system-prompt fragment for LLM agents using
`goctl`. The JSON contracts below are versioned with `schema_version: "1"`.

## Command grammar

```
goctl --env <ENV> [--region <REGION>] [--api-version <V>] <SERVICE> <COMMAND>
      [--var KEY=VALUE ...]
      [--include CATEGORY NAME ...]
      [--token <BEARER_JWT> | --token-url <URL>]
      [--config <PATH>]
      [--dry-run] [--confirm]
      [--json] [--json-errors] [--table]

goctl list-endpoints [--service <SERVICE>] [--api-version <V>] [--json]
                     [--disabled-only | --enabled-only]
goctl describe <SERVICE> <COMMAND> [--json]
goctl list-fixtures [--category <C>] [--json]
goctl describe-fixture <CATEGORY> <NAME> [--json]
```

The request body is fully controlled by `--var <name>=<value>` flags matching
the manifest's declared vars (see `goctl describe ...`). `--var <key>=@<path>`
loads the value from a file (JSON / YAML auto-detected by extension);
recommended for any non-trivial body. `--include CATEGORY NAME` is a
Captain-specific shorthand for `--var context=…` with embedded test data —
each `--include` merges `context.subject.<CATEGORY> = <fixture-json>` (the
`subject` nesting matches Captain's wire shape). Both paths are
composable: any user-supplied `--var context=…` (inline or `@file`) is the
base; `--include` overlays one category key at a time under `subject`.

For end-to-end Captain journey flows (start → device → interaction → completion)
see [`docs/captain-playbook.md`](captain-playbook.md). It explains the two-token
model (customer vs end-user), the discriminator-based ref → data-path mapping
(`PrimaryDocument/* ↔ subject.documents[].{type:"primary"}`), and the
**two-part interaction-submit** pattern (`context.subject` + `participants[]`
must be sent together).

- `ENV` ∈ `dev | demo | prod`.
- `REGION` ∈ `au | eu | us` (case-insensitive; optional when an env defines
  only one region).
- `SERVICE` ∈ `captain | designer | userview`.
- `COMMAND` is one of the manifest names. Enumerate with
  `goctl list-endpoints --service <SERVICE> --json`.
- `--api-version <V>` selects a version for a versioned service (currently:
  `captain`). Free-form string (`v1`, `v2`, future `v3`, ...). Resolution:
  explicit flag → sole supported version (if exactly one) →
  `default_version:` in `regions.yaml` → Usage error.
  (`--version` is reserved for the binary's own version, as is conventional.)

## Variable substitution

Template vars are supplied with repeated `--var KEY=VALUE`. Precedence:
`--var` > `$GGO_VAR_<UPPER_KEY>` > the manifest's `optional_vars[].default`.

Each var declares a `kind`:

- `kind: string` — passed through Tera's `json_encode` filter; arbitrary
  values with quotes, backslashes, or newlines are safely escaped.
- `kind: json` — pre-validated with `serde_json::from_str`; an invalid value
  exits 2 *before* any HTTP or auth step.

VALUE may use `@<path>` (curl convention) to read the value from a file. When
the file extension is `.yaml` / `.yml`, the contents are parsed as YAML and
re-serialized as compact JSON. To pass a literal `@` as the first character,
escape it as `\@`.

## Test fixtures (`--include`)

Captain endpoints with a `context` JSON object can be composed from built-in
fixtures via `--include CATEGORY NAME` (repeatable). Built-in categories:

| Category | Built-in names |
|---|---|
| `identity` | `testdata-v1` (UK), `testdata-v2` (US) |
| `documents` | `testdata-v1` (passport), `testdata-v2` (driver license) |
| `biometrics` | `default` |

Each `--include` JSON-merges its fixture into `context.<CATEGORY>` on top of
any user-supplied `--var context=…` base. Discover with `goctl list-fixtures`
/ inspect with `goctl describe-fixture <CATEGORY> <NAME>`. `--include` is
purely sugar for `--var context=…` — Captain manifests that don't reference
`{{ context }}` silently ignore it. Non-Captain services (Designer, UserView)
have no `context` var and so `--include` is a no-op there.

## Auth

- `--token <JWT>` (or `GGO_BEARER_TOKEN`): bypasses OAuth; the bearer is
  reused until its `exp` claim lapses (30s skew tolerance).
- Designer prod and UserView prod are bearer-only — without a bearer the
  CLI exits 2.
- Endpoints whose manifest declares `auth: none` skip the bearer-only gate
  and reach upstream with no `Authorization` header.

## Output contracts

`goctl describe <SERVICE> <COMMAND> --json` →

For **flat** (non-versioned) endpoints — e.g. anything under `designer` /
`userview`:

```json
{
  "schema_version": "1",
  "service": "userview",
  "name": "health",
  "method": "GET",
  "path": "/health",
  "description": "...",
  "auth": "none",
  "required_vars": [],
  "optional_vars": [],
  "body_template_preview": null,
  "target_url_pattern": "<base_url>/health",
  "disabled": false
}
```

For **versioned** endpoints — under `captain`:

```json
{
  "schema_version": "1",
  "service": "captain",
  "name": "journey-start",
  "description": "...",
  "auth": "bearer",
  "supported_versions": ["v1", "v2"],
  "versions": {
    "v1": {
      "method": "POST",
      "path": "/journey/start",
      "required_vars": [{"name": "journeyId", "kind": "string", "...": "..."}],
      "optional_vars": [{"name": "locale", "kind": "string", "default": "en-US"}],
      "body_template": "...",
      "disabled": false,
      "disabled_reason": null,
      "deprecated": null,
      "target_url_pattern": "<base_url:v1>/journey/start"
    },
    "v2": {
      "method": "POST",
      "path": "/journey/start",
      "required_vars": [
        {"name": "resourceId", "kind": "string", "...": "..."},
        {"name": "context", "kind": "json", "...": "..."}
      ],
      "optional_vars": [],
      "body_template": "...",
      "disabled": false,
      "disabled_reason": null,
      "deprecated": null,
      "target_url_pattern": "<base_url:v2>/journey/start"
    }
  }
}
```

`schema_version` stays `"1"` — versioned entries are additive (presence of
`supported_versions` / `versions`).

`goctl list-endpoints --service captain --json` → JSON array. Each entry
carries `{service, name, method, path, description, auth, required_vars,
disabled, supported_versions}` for versioned endpoints (and omits
`supported_versions` for flat services). Filter with
`--api-version <V>` to include only endpoints that support that version. The
`disabled` field on a versioned entry reflects "every supported version is
disabled" — to check per-version status read the `describe` output.

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
