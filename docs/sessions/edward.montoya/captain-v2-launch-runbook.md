# Launching & completing a Captain v2 journey with `goctl` — what went wrong & the correct flow

> Session runbook (2026-05-29). Goal: launch a Captain **v2** journey by `resourceId` and drive it
> to `Completed`. This captures the mistakes made on the first attempt so the flow is replicable.
> Authoritative reference: [`docs/captain-playbook.md`](../../captain-playbook.md).

Journey used: `Testing - ordering v2`
Resource: `64b1d69e99a495a51643b41bf84134ff6409e5180ec91e2c740eb60a059090ed@4hr3lwvw`
Env/region: `dev` / `eu`.

---

## TL;DR — the 5 mistakes

| # | What I did wrong | Symptom | Fix |
|---|---|---|---|
| 1 | Assumed creds auto-loaded into the tool shell | `Auth: missing env var GGO_CAPTAIN_DEV_CLIENT_ID` | `.zshrc` only auto-loads `.env` into **interactive** shells. Non-interactive/tool shells must source `.env` themselves. |
| 2 | Captain creds were absent / misnamed in `.env` | creds still missing after "fix" | Dev keys had a stray `V1_` infix (`GGO_CAPTAIN_V1_DEV_*`). Config expects `GGO_CAPTAIN_DEV_*` (no version infix). |
| 3 | Started v2 with empty `context: {}` | `400 4002 MISSING_FIELD: context.config ... expected object` | v2 **requires** `context.config.delivery` (string). Use `{"config":{"delivery":"api"}}`. |
| 4 | Submitted only `context.subject` (no `participants`) | `{"status":"success"}` but `outstanding` unchanged, nothing persisted | An `interaction-submit` needs **BOTH** `context.subject` (values) **AND** `participants[]` (the refs being satisfied). |
| 5 | Used raw fixtures for documents/biometrics | would break collect / Facematch | Documents need `type:"primary"` discriminator; biometrics must be **only** `{selfieImage}` — **no** `anchorImage`, **no** `type`. |

The big one is **#4**: subject-alone returns `success` and silently does nothing.

---

## Mistake details

### 1. Credentials don't auto-load in non-interactive shells
`~/.zshrc` defines `goctl_load_env()` and calls it on shell start — but `.zshrc` is sourced only for
**interactive** shells. Agent/tool shells (and scripts) run non-interactively, so the `GGO_*` vars
are absent and `goctl` fails OAuth. In your own terminal it works; in scripts you must load `.env`.

Reusable helper (`/tmp/gc.sh`) — loads `.env` literally (values contain `(`, `*`, `=` that break
`source`/`set -a`), then runs goctl:

```bash
#!/usr/bin/env bash
set -euo pipefail
ENV_FILE="$HOME/code/identity-gbggo-cli/.env"
while IFS= read -r line; do
  case "$line" in \#*|"") continue;; esac
  k=${line%%=*}; v=${line#*=}
  case "$k" in [A-Z0-9_]*) export "$k=$v";; esac
done < "$ENV_FILE"
exec goctl "$@"
```

### 2. `.env` key names must match `regions.yaml` exactly
`regions.yaml` references `client_id_env: GGO_CAPTAIN_DEV_CLIENT_ID`, etc. The dev secret/user/pass
had been added as `GGO_CAPTAIN_V1_DEV_*` — a name the config never looks up, so only `CLIENT_ID`
resolved. Required dev keys (no `V1_`):
`GGO_CAPTAIN_DEV_CLIENT_ID`, `GGO_CAPTAIN_DEV_CLIENT_SECRET`, `GGO_CAPTAIN_DEV_USERNAME`, `GGO_CAPTAIN_DEV_PASSWORD`.

### 3. v2 requires `context.config.delivery`
v1 makes `context.config` optional; v2 rejects a missing `config`. Minimum viable launch context is
`{"config":{"delivery":"api"}}`.

### 4. `interaction-submit` is a TWO-part body
- `context.subject` alone → `{"status":"success"}`, but the collect is **not** satisfied and the journey doesn't advance.
- `participants` alone → `400 MISSING_FIELD: context.subject`.
- **Both together** → collect satisfied, journey advances.

Each `participants` entry declares one outstanding ref: `{"domainElementId":"<ref>"}`.

### 5. Discriminators & the `anchorImage` trap
- `PrimaryDocument/<field>` → `subject.documents[]` with `{type:"primary", <field>:…}`. The built-in
  `documents` fixtures do **not** include `type`, so you must add it.
- `Selfie/<field>` → `subject.biometrics[]` as a bare `{selfieImage:…}`. **Do not** send `anchorImage`
  or a `type`: the Document-Extraction module writes the anchor as a separate biometrics entry, and
  bundling it clobbers the anchor → Facematch "System Error".
- `FullName/firstName`, `FullName/lastNames` ← read from `subject.identity.{firstName,lastNames}`.

---

## Correct replicable flow (dev/eu, customer OAuth token)

> This journey's `segment1` collects everything at once (name + document + selfie). Other journeys
> split capture across segments — loop `interaction-fetch` and submit per `outstanding` batch.

```bash
GC="$HOME/code/identity-gbggo-cli/tmp-gc.sh"   # the helper from Mistake #1
RES='64b1d69e99a495a51643b41bf84134ff6409e5180ec91e2c740eb60a059090ed@4hr3lwvw'
A=(--api-version v2 --env dev --region eu)

# 1. Start (v2 needs config.delivery)
INST=$(bash "$GC" captain journey-start "${A[@]}" \
  --var resourceId="$RES" \
  --var context='{"config":{"delivery":"api"}}' --json | jq -r .instanceId)
echo "instance=$INST"

# 2. See what's outstanding
bash "$GC" captain interaction-fetch "${A[@]}" --var instanceId="$INST" --json \
  | jq -c '{interactionId, outstanding}'

# 3. Build the two-part submit payload from fixtures
bash "$GC" describe-fixture identity  testdata-v2 > /tmp/fx_id.json
bash "$GC" describe-fixture documents testdata-v2 > /tmp/fx_doc.json
bash "$GC" describe-fixture biometrics default    > /tmp/fx_bio.json

jq -n --slurpfile id /tmp/fx_id.json --slurpfile doc /tmp/fx_doc.json --slurpfile bio /tmp/fx_bio.json '{
  subject:{
    identity:   $id[0],
    documents:  [ { type:"primary", side1Image: $doc[0][0].side1Image } ],
    biometrics: [ { selfieImage:           $bio[0][0].selfieImage } ]   # NO anchorImage / NO type
  }}' > /tmp/ctx.json

jq -n '[
  {domainElementId:"FullName/firstName"},
  {domainElementId:"FullName/lastNames"},
  {domainElementId:"PrimaryDocument/side1Image"},
  {domainElementId:"Selfie/selfieImage"}
]' > /tmp/parts.json

# 4. Submit BOTH context + participants
IID='grn:::gbg:design:interaction:segment1@latest'
bash "$GC" captain interaction-submit "${A[@]}" \
  --var instanceId="$INST" --var interactionId="$IID" \
  --var context=@/tmp/ctx.json --var participants=@/tmp/parts.json --json

# 5. Poll to terminal state (async Document-Extraction -> Facematch runs after submit)
for i in $(seq 1 12); do
  S=$(bash "$GC" captain journey-state-fetch "${A[@]}" --var instanceId="$INST" --json | jq -r .status)
  echo "[$i] status=$S"; [ "$S" = Completed ] && break; sleep 5
done
```

Done = `journey-state-fetch` → `status: Completed`, `context.result.status: complete`, and
`interaction-fetch` → `interactionId: grn:::gbg:design:interaction:end@latest`, `outstanding: []`.

---

## Quick reference

| Need | Command |
|---|---|
| List captain endpoints | `goctl list-endpoints --service captain --json` |
| Endpoint contract | `goctl describe captain <name> --api-version v2` |
| List fixtures | `goctl list-fixtures --json` |
| Inspect a fixture | `goctl describe-fixture <category> <name>` |
| Where am I? | `interaction-fetch` → `.outstanding` |
| Did it land? | `journey-state-fetch` → `.context.subject` populated + `.status` |

**Token note:** in this run the customer OAuth token (from `regions.yaml` creds) was accepted by
`interaction-fetch`/`submit`, so the `device-start`→`device-connect` end-user-JWT bootstrap in the
playbook was not needed. If those endpoints return `401`, follow the playbook's device bootstrap.
