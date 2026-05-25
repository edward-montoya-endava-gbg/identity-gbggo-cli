# goctl — GBG GO API CLI

`goctl` is a Rust CLI for invoking the GBG GO Designer, Captain v1, Captain v2,
and UserView APIs across `dev` / `demo` / `prod` and AU/EU/US regions.

## Setup overview

Getting `goctl` working on a fresh machine takes two passes:

1. **[Install](#install)** — compile the binary and put it on `$PATH`. After this, `goctl --version` works.
2. **[First-time setup](#first-time-setup-required-before-any-call)** — point the installed binary at a `regions.yaml` and export your credentials. After this, `goctl --env dev …` calls actually reach the upstream APIs.

You do (1) **once per machine**. You do (2) **once per machine** as well, but it's the step people forget — skipping it produces `config file not found` on the first call.

## Install

`goctl` is a single Rust binary. The canonical way to install it on your
machine is `cargo install`, which compiles the release binary and drops it
into `$HOME/.cargo/bin/` (which `rustup` adds to your `PATH` automatically).

### Prerequisites

- Rust toolchain (stable). If you don't have it: `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- `$HOME/.cargo/bin` on your `PATH`. `rustup` does this for you in `~/.zshrc` / `~/.bashrc`; if `which cargo` works, you're good.

### Install the binary

```
cd identity-gbggo-cli
cargo install --path .
```

That builds `--release` and installs `goctl` to `$HOME/.cargo/bin/goctl`.

Verify the binary is on `PATH`:

```
which goctl       # → /Users/<you>/.cargo/bin/goctl
goctl --version
goctl --help
```

**At this point `goctl --version` works but live calls don't yet — continue to [First-time setup](#first-time-setup-required-before-any-call) below.**

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

`goctl` needs **two** things on disk before any live call:

1. A `regions.yaml` (endpoints + OAuth2 config; **no secrets**)
2. Environment variables holding your credentials (per-service, per-env)

Skip either and your first command will exit non-zero. The two sections below walk you through both, then the Quickstart shows the result.

### 1. Point the CLI at a `regions.yaml`

Pick **one** of three options. **Option A is the recommended path** because it's identical on macOS, Linux, and WSL.

**Option A — `GOCTL_CONFIG` env var (RECOMMENDED, OS-agnostic):**

```sh
# Add to ~/.zshrc (or ~/.bashrc):
export GOCTL_CONFIG="$HOME/code/identity-gbggo-cli/regions.example.yaml"
```

Then open a fresh terminal (or `source ~/.zshrc`). The CLI will read from this path regardless of platform. Edit the file in-place when you need to change endpoints.

**Option B — copy to your OS's standard config dir:**

```sh
# macOS:
mkdir -p "$HOME/Library/Application Support/goctl"
cp regions.example.yaml "$HOME/Library/Application Support/goctl/regions.yaml"

# Linux / WSL:
mkdir -p "$HOME/.config/goctl"
cp regions.example.yaml "$HOME/.config/goctl/regions.yaml"
```

> **macOS gotcha:** the CLI uses the OS-native config directory, which on macOS is `~/Library/Application Support/goctl/` — NOT `~/.config/goctl/`. If you copied to the wrong location you'll see `config file not found at ...` pointing at the right one.

**Option C — pass `--config <PATH>` per call** (one-off testing):

```sh
goctl --config ./regions.example.yaml --env dev userview health
```

The CLI's full search order is `--config <PATH>` → `$GOCTL_CONFIG` → `$XDG_CONFIG_HOME/goctl/regions.yaml` → `$HOME/.config/goctl/regions.yaml` → OS-native config dir. First hit wins.

The config file is parsed with `deny_unknown_fields`. **Do not** paste `client_secret:`, `password:`, `token:`, or `client_id:` into it — only `*_env` references to environment variables holding the secret are accepted.

### 2. Export your credentials from `.env`

```sh
cp .env.example .env
# edit .env, fill in real values for the service+env tuples you'll use
```

**Then export with the safe loader** — `.env` values often contain `!`, `;`, `&`, `|`, `?`, `(`, `)`, `%` and similar shell metacharacters; plain `source .env` will syntax-error on these. Use this instead:

```sh
while IFS='=' read -r k v; do
  [[ "$k" =~ ^[A-Z0-9_]+$ ]] && export "$k=$v"
done < .env
```

Add it to a shell function or use `direnv` if you want this automatic per-directory. **Do not use `set -a; source .env; set +a`** — it breaks on the secrets stored here.

### 3. Verify

```sh
# auth: none — no creds needed, smoke-tests the URL chain:
goctl --env dev captain-v2 version

# requires creds — confirms .env + regions.yaml are wired up:
goctl --env dev userview journey-sessions-list --var orgId=gbg | jq '.[0]'
```

If either succeeds, you're set.

## Quickstart

After [First-time setup](#first-time-setup-required-before-any-call):

```sh
# Auth-free smoke test:
goctl --env dev captain-v2 version | jq

# List sessions in your org:
goctl --env dev userview journey-sessions-list --var orgId=gbg | jq

# Discover endpoints programmatically:
goctl list-endpoints --service userview --json | jq '.[] | {name, method, path}'
goctl describe userview journey-sessions-processing --json

# Preview a write without sending it:
goctl --env dev captain-v2 journey-start \
    --var resourceId=<your-id> --var context='{}' --dry-run
```

## Troubleshooting

| Error you see | What's wrong | Fix |
|---|---|---|
| `config file not found at .../goctl/regions.yaml` | No `regions.yaml` at any of the five searched paths. | Do [First-time setup §1](#1-point-the-cli-at-a-regionsyaml). The error message names the OS-native path; copying the file there is the most direct fix, or set `GOCTL_CONFIG`. |
| `missing env var GGO_<…>_USERNAME` (or `_PASSWORD` / `_CLIENT_ID` / `_CLIENT_SECRET`) | `.env` isn't exported in this shell, or that specific service+env tuple has no value set. | Re-run the safe-loader from [First-time setup §2](#2-export-your-credentials-from-env). Confirm with `env \| grep GGO_<service>_<env>_`. |
| `token URL responded 401 Unauthorized (invalid_grant)` | Auth flow reached Keycloak, but the username/password/client are invalid for that realm. | Verify the `.env` values for that service+env. Each service may use a different realm (Captain/Designer use `go`; UserView uses its own Keycloak at `gbggo4-dev-userview.../auth`). |
| `command <x> not found for service <y>` | Endpoint name typo. | `goctl list-endpoints --service <y>` to see the correct names. |
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
| `GGO_CAPTAIN_V1_DEV_CLIENT_ID` | Captain v1 dev OAuth2 client_id. |
| `GGO_CAPTAIN_V1_DEV_USERNAME` / `GGO_CAPTAIN_V1_DEV_PASSWORD` | Captain v1 dev (password grant). |
| `GGO_CAPTAIN_V1_DEMO_CLIENT_ID` | Captain v1 demo OAuth2 client_id. |
| `GGO_CAPTAIN_V1_DEMO_USERNAME` / `GGO_CAPTAIN_V1_DEMO_PASSWORD` | Captain v1 demo. |
| `GGO_CAPTAIN_V1_PROD_CLIENT_ID` | Captain v1 prod OAuth2 client_id. |
| `GGO_CAPTAIN_V1_PROD_USERNAME` / `GGO_CAPTAIN_V1_PROD_PASSWORD` | Captain v1 prod. |
| `GGO_CAPTAIN_V1_{DEV,DEMO,PROD}_CLIENT_SECRET` | Captain v1 OAuth2 client secret (required). |
| `GGO_CAPTAIN_V2_DEV_CLIENT_ID` | Captain v2 dev OAuth2 client_id. |
| `GGO_CAPTAIN_V2_DEV_USERNAME` / `GGO_CAPTAIN_V2_DEV_PASSWORD` | Captain v2 dev. |
| `GGO_CAPTAIN_V2_DEMO_CLIENT_ID` | Captain v2 demo OAuth2 client_id. |
| `GGO_CAPTAIN_V2_DEMO_USERNAME` / `GGO_CAPTAIN_V2_DEMO_PASSWORD` | Captain v2 demo. |
| `GGO_CAPTAIN_V2_PROD_CLIENT_ID` | Captain v2 prod OAuth2 client_id. |
| `GGO_CAPTAIN_V2_PROD_USERNAME` / `GGO_CAPTAIN_V2_PROD_PASSWORD` | Captain v2 prod. |
| `GGO_CAPTAIN_V2_{DEV,DEMO,PROD}_CLIENT_SECRET` | Captain v2 OAuth2 client secret (required). |
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

Captain v1 and Captain v2 prod use the `password` OAuth2 grant — credentials
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

- `goctl list-endpoints [--service <SVC>] --json` returns the full catalog
  as a JSON array. Each entry has `{service, name, method, path, description,
  auth, required_vars}`.
- `goctl describe <service> <command> --json` returns the full manifest as a
  JSON object: `{schema_version: "1", service, name, method, path, description,
  auth, required_vars, optional_vars, body_template_preview, target_url_pattern}`.
- `goctl --json-errors ...` emits machine-readable errors on stderr.
- See `docs/llm-usage.md` for a copy-pasteable system-prompt fragment.

The command grammar is stable:

```
goctl --env <ENV> [--region <R>] <SERVICE> <COMMAND>
  [--var KEY=VALUE ...]
  [--token <T> | --token-url <U>]
  [--dry-run] [--confirm]
  [--json] [--json-errors] [--table]
```
