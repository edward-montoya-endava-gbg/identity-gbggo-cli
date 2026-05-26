# goctl — GBG GO API CLI

`goctl` is a Rust CLI for invoking the GBG GO Designer, Captain (versioned),
and UserView APIs across `dev` / `demo` / `prod` and AU/EU/US regions.

## Setup overview

Getting `goctl` working on a fresh machine takes two passes:

1. **[Install](#install)** — compile the binary and put it on `$PATH`.
2. **[First-time setup](#first-time-setup-required-before-any-call)** — point the installed binary at a `regions.yaml` and export your credentials. After this, `goctl --env dev …` calls actually reach the upstream APIs.

You do (1) **once per machine**. You do (2) **once per machine** as well, but it's the step people forget — skipping it produces `config file not found` on the first call.

> **Migration note (2026-05-25):** Captain v1 and Captain v2 are now one CLI service named `captain`. The API-version flag is `--api-version` (the bare `--version` prints the binary version, as expected); pass `--api-version v1` / `--api-version v2` to select the wire shape. If your existing `.env` uses `GGO_CAPTAIN_V1_*` / `GGO_CAPTAIN_V2_*`, rename them to `GGO_CAPTAIN_*` (drop the version suffix) — see [`.env.example`](.env.example). The CLI subcommand `goctl captain-v1 …` / `goctl captain-v2 …` is gone; use `goctl captain <command> [--api-version <v>]` instead.

## Install

`goctl` is a single Rust binary. The canonical way to install it on your
machine is `cargo install`, which compiles the release binary and drops it
into `$HOME/.cargo/bin/`.

### Prerequisites

- Rust toolchain (stable). If you don't have it: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- `$HOME/.cargo/bin` on your `PATH`.

### Install the binary

```
cd identity-gbggo-cli
cargo install --path .
```

That builds `--release` and installs `goctl` to `$HOME/.cargo/bin/goctl`.

Verify the binary is on `PATH`:

```
which goctl       # → /Users/<you>/.cargo/bin/goctl
goctl --help
```

**At this point the binary is installed but live calls don't work yet — continue to [First-time setup](#first-time-setup-required-before-any-call) below.**

### Update

Re-run the install with `--force` to overwrite the existing binary after pulling new code:

```
cd identity-gbggo-cli
git pull
cargo install --path . --force
```

### Uninstall

```
cargo uninstall goctl
```

That removes the binary from `$HOME/.cargo/bin/`. Your `regions.yaml` and `.env` are untouched — delete those manually if you want a clean wipe:

```sh
# macOS
rm -rf "$HOME/Library/Application Support/goctl"
# Linux
rm -rf "$HOME/.config/goctl"
```

## First-time setup (REQUIRED before any call)

> **Migration:** if your existing `.env` still defines `GGO_CAPTAIN_V1_*` / `GGO_CAPTAIN_V2_*`, rename them to `GGO_CAPTAIN_*` (drop the version suffix), or add aliases that mirror the new names. See [`.env.example`](.env.example) for the full list.

`goctl` needs **two** things to run a live call:

1. A `regions.yaml` (endpoints + OAuth2 config; **no secrets**)
2. Environment variables holding your credentials (per-service, per-env)

Skip either and your first command will exit non-zero. The recommended path runs **one script** in this repo and wires both up for every future terminal session — no per-session sourcing.

### Recommended: `./setup-shell.sh` (one command, every session)

```sh
# 1. Create your real config files in the repo:
cp regions.example.yaml regions.yaml   # edit if you need to change endpoints
cp .env.example .env                   # fill in real values for the (service, env) tuples you'll use

# 2. Wire them into your shell — once:
./setup-shell.sh
```

That appends a small managed block to `~/.zshrc` (or `~/.bashrc` / `~/.bash_profile`) that:
- Exports `GOCTL_CONFIG` pointing at this repo's `regions.yaml`.
- Defines a `goctl_load_env` function that safely exports every `GGO_*` variable from this repo's `.env` (handles values containing `!`, `;`, `&`, `|`, `?`, `(`, `)`, `%`, etc.).
- Auto-runs `goctl_load_env` on every new shell, so `goctl` is ready immediately when you open a terminal.

```sh
# Activate the current terminal (new ones are automatic):
source ~/.zshrc

# After editing .env, reload without opening a fresh terminal:
goctl_load_env

# Remove the managed block later:
./setup-shell.sh --uninstall
```

The script is **idempotent** — re-running it replaces the previous block in place, picking up any path changes if you've moved the repo.

> **macOS gotcha (if you skip the script):** the CLI's OS-native config fallback on macOS is `~/Library/Application Support/goctl/`, NOT `~/.config/goctl/`. Using the script avoids this entirely by setting `GOCTL_CONFIG` to an absolute path.

The config file is parsed with `deny_unknown_fields`. **Do not** paste `client_secret:`, `password:`, `token:`, or `client_id:` into it — only `*_env` references to environment variables holding the secret are accepted.

### Verify

```sh
# auth: none — no creds needed, smoke-tests the URL chain (Captain v2):
goctl --env dev captain version

# requires creds — confirms .env + regions.yaml are wired up:
goctl --env dev userview journey-sessions-list --var orgId=gbg | jq '.records[0]'
```

If either succeeds, you're set.

### Manual alternatives (skip if you ran `setup-shell.sh`)

The CLI's config search order is `--config <PATH>` → `$GOCTL_CONFIG` → `$XDG_CONFIG_HOME/goctl/regions.yaml` → `$HOME/.config/goctl/regions.yaml` → OS-native config dir. First hit wins. Pick any of:

**Manual A — set `GOCTL_CONFIG` yourself:**

```sh
echo 'export GOCTL_CONFIG="$HOME/code/identity-gbggo-cli/regions.yaml"' >> ~/.zshrc
```

**Manual B — copy `regions.yaml` to the OS-native config dir:**

```sh
# macOS:
mkdir -p "$HOME/Library/Application Support/goctl"
cp regions.example.yaml "$HOME/Library/Application Support/goctl/regions.yaml"

# Linux / WSL:
mkdir -p "$HOME/.config/goctl"
cp regions.example.yaml "$HOME/.config/goctl/regions.yaml"
```

**Manual C — pass `--config <PATH>` per call** (one-off testing):

```sh
goctl --config ./regions.yaml --env dev userview health
```

**For credentials**, if you don't use `setup-shell.sh`, use this safe loader (NOT `set -a; source .env; set +a` — that breaks on special chars):

```sh
while IFS='=' read -r k v; do
  [[ "$k" =~ ^[A-Z0-9_]+$ ]] && export "$k=$v"
done < .env
```

## Captain v1 and Captain v2 live under one service

> 📘 **For end-to-end how-tos, read [`docs/captain-playbook.md`](docs/captain-playbook.md).** It covers the full v1 (single-call) and v2 (start → device-bootstrap → interaction-loop) flows with copy-pasteable commands, the discriminator → data-path mapping table, and every gotcha we hit while bringing both up.

Captain ships as one CLI service named `captain` with an `--api-version` flag. Each
endpoint manifest declares the set of versions it supports
(`supported_versions:`) plus a per-version block holding the method, path,
required vars, and body template for that version. Adding v3 later is a pure
manifest + `regions.yaml` edit — no CLI rebuild is needed for callers on v1/v2.

Resolution order for `--api-version`:

1. Explicit `--api-version <V>` flag.
2. If the endpoint has exactly one supported version, it is inferred (this
   wins over `default_version` — there's no real ambiguity to disambiguate).
3. `default_version:` configured for the env in `regions.yaml` (only consulted
   when the endpoint supports multiple versions).
4. Otherwise the CLI exits Usage (2) listing the supported versions.

Examples:

```sh
# v2-only endpoint; --api-version is inferred:
goctl --env dev captain version

# v1-only endpoint; --api-version is inferred to v1:
goctl --env dev captain journey-task-update \
  --var journeyId=... --var taskId=... --var intent=Validate --var data='{}'

# Shared endpoint; pick the wire shape with --api-version:
goctl --env dev captain journey-start \
  --api-version v1 --var journeyId=... --var externalId=ext-1

goctl --env dev captain journey-start \
  --api-version v2 --var resourceId=...@latest --var context='{}'
```

`regions.example.yaml` ships with `default_version: v2` for each env, so calls
to shared (multi-version) endpoints default to v2 when `--api-version` is
omitted.

## Quickstart

After [First-time setup](#first-time-setup-required-before-any-call):

```sh
# Auth-free smoke test (Captain v2):
goctl --env dev captain version | jq

# List sessions in your org:
goctl --env dev userview journey-sessions-list --var orgId=gbg | jq

# Discover endpoints programmatically:
goctl list-endpoints --service userview --json | jq '.[] | {name, method, path}'
goctl list-endpoints --service captain --api-version v2 --json | jq '.[].name'
goctl describe captain journey-start --json

# Preview a write without sending it:
goctl --env dev captain journey-start \
    --api-version v2 --var resourceId=<your-id> --var context='{}' --dry-run
```

## Test fixtures (Captain payloads made easy)

Captain endpoints take a `context` JSON object. The canonical way to supply it
is **as a file via the `@` prefix** — curl convention, full control, no shell-quoting headaches:

```sh
# 1. Write your payload to a file (JSON or YAML — auto-detected by extension):
cat > /tmp/journey-context.json <<'EOF'
{
  "identity": { "firstName": "Sean", "lastName": "Martin" },
  "locale": "en-US"
}
EOF

# 2. Pass it via @file:
goctl --env dev captain --api-version v2 journey-start \
  --var resourceId=<your-resource-id> \
  --var context=@/tmp/journey-context.json
```

Inline JSON via `--var context='{...}'` still works for short ad-hoc values:

```sh
goctl --env dev captain --api-version v2 journey-start \
  --var resourceId=<id> \
  --var context='{"locale":"en-US"}'
```

Both paths are first-class — the rendered body is identical to today's
behavior; `--var` (inline or `@file`) is the only thing controlling the body
shape unless you opt into `--include`.

### Shortcut — built-in fixtures via `--include`

For common test scenarios, `--include CATEGORY NAME` grabs a pre-built fixture
and merges it into `context.<CATEGORY>` without you crafting JSON each time:

```sh
goctl --env dev captain --api-version v2 journey-start \
  --var resourceId=<your-resource-id> \
  --include identity testdata-v1 \
  --include documents testdata-v2 \
  --include biometrics default
```

That renders a request body where `context = { identity: <fixture>, documents:
<fixture>, biometrics: <fixture> }`.

Combine with raw JSON or `@file` — the user-supplied `context` is the base,
each `--include` overlays one key:

```sh
goctl --env dev captain --api-version v2 journey-start \
  --var resourceId=<id> \
  --var context='{"locale":"en-US","region":"EU"}' \
  --include identity testdata-v1
# Final context: {"locale":"en-US","region":"EU","identity":<fixture>}
```

### Discover fixtures

```sh
goctl list-fixtures
goctl list-fixtures --category biometrics --json
goctl describe-fixture identity testdata-v1
```

### Available built-in categories

| Category | Built-in fixtures | What they contain |
|---|---|---|
| `identity` | `testdata-v1` (UK), `testdata-v2` (US) | First name, last name, DOB, address |
| `documents` | `testdata-v1` (passport), `testdata-v2` (driver license) | Document type, country, image refs |
| `biometrics` | `default` | `selfieImage` (`@file:images/faceImage.b64`), `anchorImage` (`@file:images/document.b64`) |

### Add a custom fixture

1. Drop a YAML file at `src/fixtures/<category>/<name>.yaml`.
2. To reference an image binary: use `"@file:images/<your-image>.b64"` —
   base64-encoded files go in `src/fixtures/images/`.
3. `cargo install --path . --force` to rebuild.
4. Use it: `--include <category> <name>`.

### Use a one-off fixture without baking it in

Pass a path (contains `/` or `.`) instead of a built-in name:

```sh
goctl --env dev captain --api-version v2 journey-start \
  --var resourceId=<id> \
  --include identity /tmp/my-custom-identity.yaml
```

### Replacing the placeholder images

The two `.b64` files at `src/fixtures/images/` ship as placeholders. Replace
their contents with real base64-encoded JPEG/PNG bytes (e.g. `base64 -i selfie.jpg > src/fixtures/images/faceImage.b64`),
then `cargo install --path . --force`.

## Disabled endpoints

Several Designer and UserView endpoints are **disabled by default** because
they modify upstream state and need more verification before live use. Running
any of them exits `Usage (2)` immediately:

> ``endpoint <service>/<name> is disabled: pending verification — write endpoint, needs more testing before live use. To enable, edit src/endpoints/<service>/<name>.yaml and remove `disabled: true`, then run `cargo install --path . --force`.``

For Captain, the disabled gate is **per-version**: a `disabled: true` lives
inside `versions.<v>:` and the error message instructs the operator to remove
it under that specific version. A Captain endpoint is treated as "fully
disabled" only when every supported version is off.

| Service | Disabled |
|---|---|
| `designer` | journey-revisions-create, create-journey, create-journey-from-template, create-interaction-journey, archive-journey, unarchive-journey, link-journey-to-child, unlink-journey, delivery-revert, update-org-settings, add-license, launch-flow, image-draft-get |
| `userview` | journey-sessions-delete, journey-sessions-change-decision, audit-logs-create, preferences-ui-put |

Captain write endpoints (v1 and v2) remain enabled in this pass.

To list the current set programmatically:

```sh
goctl list-endpoints --disabled-only --json | jq '.[] | {service, name, disabled_reason}'
```

To enable one flat (non-versioned) endpoint for testing:

1. Edit `src/endpoints/<service>/<name>.yaml` and delete the `disabled: true` and `disabled_reason:` lines.
2. `cargo install --path . --force` to rebuild the binary.
3. Run the endpoint normally.

To enable a single Captain version (e.g. unblock v1 only) edit
`src/endpoints/captain/<name>.yaml` and remove `disabled: true` from the
`versions.v1:` block, then re-install. Other versions remain unaffected.

## Adding a new Captain version (e.g. v3)

1. Add a `v3:` block to each shared `src/endpoints/captain/<endpoint>.yaml`
   that should support it (and create new manifests for v3-only endpoints).
   Append `v3` to the file's `supported_versions:` list.
2. Add `v3:` entries to `regions.yaml` under
   `captain.envs.<env>.regions.<r>.base_urls`.
3. (Optional) flip `default_version: v3` for the env once you want it to be
   the default.
4. `cargo install --path . --force`.

Callers on v1/v2 keep working — they never had to rebuild because their
explicit `--api-version` (or `default_version`) still resolves.

## Troubleshooting

| Error you see | What's wrong | Fix |
|---|---|---|
| `config file not found at .../goctl/regions.yaml` | No `regions.yaml` at any of the five searched paths. | Do [First-time setup](#first-time-setup-required-before-any-call). The error message names the OS-native path; copying the file there is the most direct fix, or set `GOCTL_CONFIG`. |
| `missing env var GGO_<…>_USERNAME` (or `_PASSWORD` / `_CLIENT_ID` / `_CLIENT_SECRET`) | `.env` isn't exported in this shell, or that specific service+env tuple has no value set. | Re-run the safe-loader from [First-time setup](#first-time-setup-required-before-any-call). Confirm with `env \| grep GGO_<service>_<env>_`. |
| `token URL responded 401 Unauthorized (invalid_grant)` | Auth flow reached Keycloak, but the username/password/client are invalid for that realm. | Verify the `.env` values for that service+env. Each service may use a different realm (Captain/Designer use `go`; UserView uses its own Keycloak at `gbggo4-dev-userview.../auth`). |
| `command <x> not found for service <y>` | Endpoint name typo. | `goctl list-endpoints --service <y>` to see the correct names. |
| `endpoint captain/<name> exists in versions [v1, v2]; pass --api-version or set default_version in regions.yaml` | Shared Captain endpoint invoked without a version hint. | Pass `--api-version v1` / `--api-version v2`, or set `default_version:` for the env in `regions.yaml`. |
| `endpoint captain/<name> does not support version v<X>` | The version doesn't exist on this endpoint. | `goctl describe captain <name> --json` to see `supported_versions`. |
| `regions.yaml captain.<env>.regions.<r>.base_urls has no entry for version v<X>` | The env's region map omits the version. | Add `vX: <url>` under `base_urls` in `regions.yaml`. |
| `<service> in prod is bearer-only; supply --token or GGO_BEARER_TOKEN` | Designer + UserView prod don't have an OAuth flow configured (by design). | `export GGO_BEARER_TOKEN=<your-bearer>` or pass `--token <jwt>`. |
| `--var <key>` parses but the body is wrong / upstream 400s | `--var` values containing `"` / `\` / newlines mid-string. | The CLI escapes string vars with `json_encode`, but values still need to be valid Bash/zsh tokens — wrap with single quotes: `--var notes='multi "word" value'`. |

For machine-readable errors in scripts, append `--json-errors`:

```sh
goctl --env dev userview journey-sessions-processing --var orgId=gbg --var id=foo --json-errors
# stderr: {"schema_version":"1","exit_code":4,"kind":"UpstreamClient","message":"...","context":{...}}
```

## Env vars

Default env-var names are per-(service, environment). The actual name read at
runtime is whatever `regions.yaml` declares in `*_env` — operators can rename.

Every callable (env, service) tuple uses the `password` grant — `flow-api-test`
and `userview-full` are Keycloak users — so each tuple needs `USERNAME`,
`PASSWORD`, **and** `CLIENT_SECRET` (every client in this org is confidential;
the CLI rejects configs that omit `client_secret_env`).

| Var | Purpose |
|---|---|
| `GGO_CAPTAIN_DEV_CLIENT_ID` | Captain dev OAuth2 client_id. |
| `GGO_CAPTAIN_DEV_USERNAME` / `GGO_CAPTAIN_DEV_PASSWORD` | Captain dev (password grant). |
| `GGO_CAPTAIN_DEMO_CLIENT_ID` | Captain demo OAuth2 client_id. |
| `GGO_CAPTAIN_DEMO_USERNAME` / `GGO_CAPTAIN_DEMO_PASSWORD` | Captain demo. |
| `GGO_CAPTAIN_PROD_CLIENT_ID` | Captain prod OAuth2 client_id. |
| `GGO_CAPTAIN_PROD_USERNAME` / `GGO_CAPTAIN_PROD_PASSWORD` | Captain prod. |
| `GGO_CAPTAIN_{DEV,DEMO,PROD}_CLIENT_SECRET` | Captain OAuth2 client secret (required). |
| `GGO_DESIGNER_DEV_CLIENT_ID` | Designer dev OAuth2 client_id. |
| `GGO_DESIGNER_DEV_USERNAME` / `GGO_DESIGNER_DEV_PASSWORD` | Designer dev. |
| `GGO_DESIGNER_DEMO_CLIENT_ID` | Designer demo OAuth2 client_id. |
| `GGO_DESIGNER_DEMO_USERNAME` / `GGO_DESIGNER_DEMO_PASSWORD` | Designer demo. |
| `GGO_DESIGNER_{DEV,DEMO}_CLIENT_SECRET` | Designer OAuth2 client secret (required, dev/demo). |
| Designer prod | bearer-only — no env vars; supply `--token` / `GGO_BEARER_TOKEN`. |
| `GGO_USERVIEW_DEV_CLIENT_ID` | UserView dev OAuth2 client_id. |
| `GGO_USERVIEW_DEV_USERNAME` / `GGO_USERVIEW_DEV_PASSWORD` | UserView dev. |
| `GGO_USERVIEW_DEMO_CLIENT_ID` | UserView demo OAuth2 client_id. |
| `GGO_USERVIEW_DEMO_USERNAME` / `GGO_USERVIEW_DEMO_PASSWORD` | UserView demo. |
| `GGO_USERVIEW_{DEV,DEMO}_CLIENT_SECRET` | UserView OAuth2 client secret (required, dev/demo). |
| UserView prod | bearer-only — no env vars; supply `--token` / `GGO_BEARER_TOKEN`. |
| `GGO_BEARER_TOKEN` | Caller-supplied bearer (skips OAuth). Any service / env. |
| `GGO_VAR_<UPPER_KEY>` | Default for template var `<key>`. |
| `GOCTL_CONFIG` | Override `regions.yaml` path. |

## Prod gate

Designer and UserView in prod are **bearer-only** (`auth: null` in
`regions.yaml`). The CLI runs no OAuth flow for them. Without `--token` /
`GGO_BEARER_TOKEN`, the CLI exits 2 before any network/auth/env-var read.

Captain prod (any version) uses the `password` OAuth2 grant — credentials
sourced via the env vars named in `regions.yaml`.

Endpoints whose manifest declares `auth: none` (health, device-connect)
bypass both the bearer-only gate and the OAuth flow.

## Exit codes

| Code | Meaning |
|---|---|
| 0 | Success |
| 1 | Config error (malformed regions.yaml or manifest) |
| 2 | Usage error (missing flag, missing var, bearer-only without bearer, etc.) |
| 3 | Auth error |
| 4 | Upstream 4xx |
| 5 | Upstream 5xx or network failure |

`--json-errors` emits a single-line JSON object on stderr matching
`{schema_version, exit_code, kind, message, context}`.

## For LLM agents

`goctl` is designed to be machine-discoverable.

- `goctl list-endpoints [--service <SVC>] [--api-version <V>] --json` returns the full catalog
  as a JSON array. Each entry has `{service, name, method, path, description,
  auth, required_vars, disabled, supported_versions?}`. `supported_versions` is
  present only for versioned services (currently: `captain`).
- `goctl describe <service> <command> --json` returns the full manifest as a
  JSON object. Flat endpoints emit `{schema_version, service, name, method,
  path, description, auth, required_vars, optional_vars, body_template_preview,
  target_url_pattern}`. Versioned endpoints emit `{schema_version, service,
  name, description, auth, supported_versions, versions: { v1: {...}, v2: {...} }}`
  where each per-version block carries its own `method`, `path`, vars, body
  template, disabled/deprecated flags, and `target_url_pattern`.
- `goctl --json-errors ...` emits machine-readable errors on stderr.
- See `docs/llm-usage.md` for a copy-pasteable system-prompt fragment.

The command grammar is stable:

```
goctl --env <ENV> [--region <R>] [--api-version <V>] <SERVICE> <COMMAND>
  [--var KEY=VALUE ...]
  [--token <T> | --token-url <U>]
  [--dry-run] [--confirm]
  [--json] [--json-errors] [--table]
```
