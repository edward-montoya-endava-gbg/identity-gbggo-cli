# Captain journey playbook

End-to-end how-to for running real Captain journeys against `dev` (and beyond)
via `goctl`. Captures the body shapes, the two-token model, the interaction
submit protocol, and every gotcha we hit while bringing both v1 and v2 up.

This is the document to read **before** you try to drive a journey yourself.

---

## TL;DR — what each version looks like

| | **v1** | **v2** |
|---|---|---|
| When to use | The journey's `resourceId` ends with a v1 version (e.g. ends in `@1atcr6m7`) | The journey's `resourceId` ends with a v2 version (e.g. ends in `@m49aitfa`) |
| Body of `journey-start` | `{ resourceId, context }` | `{ resourceId, context }` — same |
| `context.config` | Optional | **Required** — must contain `{ delivery: <string> }` |
| Sync or async? | Synchronous — `journey-start` returns once the journey is `Completed` | Asynchronous — `journey-start` returns immediately with `status: started` / `InProgress`, you drive it via `interaction-*` until `Completed` |
| End-user token needed? | No | Yes — bootstrap via `device-start` → `device-connect` |

You always:

1. Have a published `resourceId` from Designer.
2. Have the **customer credentials** for Captain (see `.env` `GGO_CAPTAIN_<ENV>_*`).
3. Optionally, build up a `context.subject` payload — by hand via `--var context=@file.json`, or via fixtures (`--include identity ...`, `--include documents ...`, `--include biometrics ...`).

---

## Prerequisites

1. `goctl` installed (see `README.md` → Install).
2. `regions.yaml` in your XDG config dir (or pointed at via `$GOCTL_CONFIG`) with the `captain` service block.
3. `.env` exported with the unified Captain env vars:
   ```
   GGO_CAPTAIN_DEV_CLIENT_ID=…
   GGO_CAPTAIN_DEV_CLIENT_SECRET=…
   GGO_CAPTAIN_DEV_USERNAME=…
   GGO_CAPTAIN_DEV_PASSWORD=…
   ```
   (If your `.env` still has the old `GGO_CAPTAIN_V1_*` / `GGO_CAPTAIN_V2_*` split, alias them to the unified names — the values are typically identical between V1 and V2 for the same env. See [README → Env vars](../README.md#env-vars).)
4. The base64 fixture images at `src/fixtures/images/faceImage.b64` and `src/fixtures/images/document.b64` populated with real bytes. `cargo install --path . --force` after replacing them so the binary picks them up.

Quick smoke that everything's wired:

```sh
goctl --env dev captain version   # auth:none — no creds needed
goctl --env dev captain --api-version v1 journey-state-fetch --var instanceId=does-not-exist  # exits 4 (404) but proves auth works
```

---

## Running a v1 journey end-to-end

v1 is synchronous: one POST, journey completes inside the same request (or returns an error). The fixture-based form covers most happy-path tests:

```sh
goctl --env dev captain --api-version v1 journey-start \
  --var resourceId=<your-v1-resource>@<version> \
  --var context='{}' \
  --include identity testdata-v1 \
  --include documents testdata-v1 \
  --include biometrics default
```

What this builds and sends:

```
POST <captain-base>/captain/api/journey/start
Authorization: Bearer <customer JWT>
Content-Type: application/json

{
  "resourceId": "<your-v1-resource>@<version>",
  "context": {
    "subject": {
      "identity":   <fixture: identity/testdata-v1>,
      "documents":  <fixture: documents/testdata-v1>,
      "biometrics": <fixture: biometrics/default>
    }
  }
}
```

Response (when the journey completes synchronously):

```json
{ "instanceId": "PiK…" }
```

Confirm:

```sh
goctl --env dev captain --api-version v1 journey-state-fetch --var instanceId=<from-above>
# → { "instanceId": "...", "status": "Completed", "metaData": {...} }
```

### Want full control over the body?

Skip `--include` entirely and pass a hand-built JSON file:

```sh
goctl --env dev captain --api-version v1 journey-start \
  --var resourceId=<id>@<version> \
  --var context=@./my-context.json
```

`--var context=@<path>` reads the file at that path (JSON or YAML; YAML auto-converted). This is the canonical full-control mode. `--include` is just sugar that overlays specific keys into `context.subject.<category>`.

---

## Running a v2 journey end-to-end

v2 is asynchronous. The CLI sequence is:

1. `journey-start` → receive `instanceId` (status `InProgress`).
2. `device-start` → receive a `connectToken` (one-time, ~2-minute TTL).
3. `device-connect` → exchange the `connectToken` for an `endUserToken` (HS256 JWT, ~24-hour TTL).
4. Loop:
   - `interaction-fetch` (with `--token <endUserToken>`) → see what the journey is currently asking for.
   - `interaction-submit` (with `--token <endUserToken>`) → provide the data.
5. Until `journey-state-fetch` reports `status: Completed`.

### Step 1 — start

The big difference vs v1: **`context.config.delivery` is required.** Without it, you get a `400 MISSING_FIELD: context.config: Invalid input: expected object, received undefined`.

```sh
goctl --env dev captain --api-version v2 journey-start \
  --var resourceId=<your-v2-resource>@<version> \
  --var context='{"config":{"delivery":"api"}}' \
  --include identity testdata-v1 \
  --include documents testdata-v1 \
  --include biometrics default
```

Response:

```json
{
  "instanceId": "0_YnpSEXaVxEV7U_ldMTcF",
  "status": "started",
  "message": "Journey 0_YnpSEXaVxEV7U_ldMTcF started successfully"
}
```

Hang onto `instanceId`.

> **Why `delivery: "api"`?** It identifies the delivery channel for this journey instance. `"api"` is the value the test environment expects for direct-API drivers. Other deployments may use a tenant-specific delivery slug — confirm with ops.

### Step 2 — bootstrap the end-user token

`interaction-*` endpoints reject the customer token. They need a separate **end-user token** (HS256 JWT issued by Captain), bootstrapped via the device exchange.

```sh
INSTANCE=0_YnpSEXaVxEV7U_ldMTcF

# 2a. Get a one-time connect token
CONNECT=$(goctl --env dev captain --api-version v2 device-start \
  --var instanceId="$INSTANCE" | jq -r .connectToken)

# 2b. Exchange it for the end-user token.
# device-connect's current contract expects the connect token in the
# Authorization: Bearer header, NOT the request body. The goctl CLI's
# --token flag currently validates JWT shape, so this one call uses curl:
END_USER_JWT=$(curl -s -X POST \
  "https://<captain-host>/v2/captain/journey/device/connect" \
  -H "Authorization: Bearer $CONNECT" \
  -H "Content-Type: application/json" \
  --data '{}' | jq -r .endUserToken)

# The CLI manifest is being updated to support raw bearer values; until then,
# the curl-bypass is the documented workaround.
```

The end-user JWT is good for ~24h and grants UPDATE on this `instanceId` only. Pass it to subsequent `interaction-*` calls via `--token`.

### Step 3 — the interaction loop

#### See what's outstanding

```sh
goctl --env dev captain --api-version v2 interaction-fetch \
  --var instanceId="$INSTANCE" \
  --token "$END_USER_JWT" \
  | jq '{interactionId, outstanding, requiredCollects: [.interaction.collects[]? | select(.spec=="required") | .ref], currentPage: .interaction.resource.data.pages[0].label}'
```

Example response (segment 1, the document capture step):

```json
{
  "interactionId": "grn:::gbg:design:interaction:segment1@latest",
  "outstanding": ["PrimaryDocument/side1Image"],
  "requiredCollects": ["PrimaryDocument/side1Image"],
  "currentPage": "Document Capture"
}
```

The `outstanding` array tells you exactly what to submit next.

#### Translate `outstanding` refs into a submit payload

This is the part that's non-obvious. **Each `outstanding` ref maps to a discriminated path in `context.subject`**:

| `outstanding` ref pattern | Where the value lives in `context.subject` |
|---|---|
| `PrimaryDocument/<field>` | `documents[ where type=="primary" ].<field>` |
| `SecondaryDocument/<field>` (when present) | `documents[ where type=="secondary" ].<field>` |
| `Selfie/<field>` | `biometrics[ where type=="selfie" ].<field>` |
| `Subject/<field>` (e.g. `Subject/identity`) | `identity.<field>` (no discriminator — it's a singleton) |

So `PrimaryDocument/side1Image` → `subject.documents[0]` with `{ type: "primary", side1Image: <value> }`.

The discriminator mechanism is implemented by the journey interpreter's `createDocumentEntry({id, discriminator})` calls (`packages/interpreter` source — see `subject-merge.spec.ts` for canonical examples). Refs with the same slot name (e.g. `PrimaryDocument/type` + `PrimaryDocument/side1Image`) merge onto the same array element.

#### Submit — the **two-part** body

This is the single most important pattern in the playbook:

> **An interaction-submit needs BOTH `context.subject` (the values) AND `participants[]` (the declaration of which domain-element refs are being submitted).**
>
> - Submit `context.subject` alone → request returns `{"status":"success"}` but the collect is silently NOT satisfied. The journey doesn't advance.
> - Submit `participants` alone → 400 `MISSING_FIELD: context.subject`.
> - Submit both together → collect satisfied, journey advances.

Build the two payloads with `jq --rawfile` (which handles MB-scale base64 cleanly):

```sh
# Context with the value, discriminator-keyed
jq -n --rawfile doc src/fixtures/images/document.b64 '{
  subject: {
    documents: [
      { "type": "primary", "side1Image": ($doc | rtrimstr("\n")) }
    ]
  }
}' > /tmp/ctx.json

# Participants declaring which ref is being submitted
jq -n '[ {"domainElementId": "PrimaryDocument/side1Image"} ]' > /tmp/parts.json

# Submit
goctl --env dev captain --api-version v2 interaction-submit \
  --var instanceId="$INSTANCE" \
  --var interactionId="grn:::gbg:design:interaction:segment1@latest" \
  --var context=@/tmp/ctx.json \
  --var participants=@/tmp/parts.json \
  --token "$END_USER_JWT"
# → {"status":"success"}
```

After this, `interaction-fetch` should report a new `interactionId` and new `outstanding` — the journey advanced.

#### Repeat until done

For the canonical biometrics-bearing dev journey we ran:

| Step | `outstanding` | What to submit |
|---|---|---|
| segment1 — Document Capture | `PrimaryDocument/side1Image` | `subject.documents = [{type:"primary", side1Image:<doc-b64>}]` + participants |
| segment4 — Biometrics Capture | `Selfie/selfieImage` | `subject.biometrics = [{selfieImage:<face-b64>}]` + participants |

> 📘 **`anchorImage` is NOT supplied by the caller.** The Document Extraction module extracts the face from the document you submitted as `PrimaryDocument/side1Image` and stores it as a SEPARATE biometrics entry: `biometrics: [{anchorImage: "gofs://…"}, {selfieImage: "…"}]`. Submitting `{type:"selfie", selfieImage:<face>, anchorImage:<doc>}` is wrong on two counts — there is no `type` discriminator for biometrics, and bundling `anchorImage` with the selfie clobbers the extraction-populated anchor. Just send `{selfieImage:<b64>}` and let the prior Document Extraction module supply the anchor.

After the selfie submit, `journey-state-fetch` returns `status: Completed` and Facematch compares your `selfieImage` against the anchor face that Document Extraction wrote.

---

## Worked example — full v2 script

A complete, copy-pasteable script that drives a v2 journey from start to completion against dev:

```sh
#!/usr/bin/env bash
set -euo pipefail

RESOURCE="91608159a6382f4e7c61e6a0471c54a05e827a5c544abc575dd3e25dd23955c2@m49aitfa"
CAPTAIN_HOST="https://gbggo4-dev.nonprod.fabric.gbgplatforms.com"
DOC_B64="src/fixtures/images/document.b64"
FACE_B64="src/fixtures/images/faceImage.b64"

# 1. Start
INSTANCE=$(goctl --env dev captain --api-version v2 journey-start \
  --var resourceId="$RESOURCE" \
  --var context='{"config":{"delivery":"api"}}' \
  --include identity testdata-v1 \
  --include documents testdata-v1 \
  --include biometrics default \
  | jq -r .instanceId)
echo "instanceId=$INSTANCE"

# 2. Bootstrap end-user JWT
CONNECT=$(goctl --env dev captain --api-version v2 device-start \
  --var instanceId="$INSTANCE" | jq -r .connectToken)
END_USER_JWT=$(curl -s -X POST "$CAPTAIN_HOST/v2/captain/journey/device/connect" \
  -H "Authorization: Bearer $CONNECT" \
  -H "Content-Type: application/json" --data '{}' \
  | jq -r .endUserToken)

# 3. Loop until done
while :; do
  STATE=$(goctl --env dev captain --api-version v2 interaction-fetch \
    --var instanceId="$INSTANCE" --token "$END_USER_JWT")
  OUTSTANDING=$(echo "$STATE" | jq -r '.outstanding // [] | join(",")')
  INTID=$(echo "$STATE" | jq -r '.interactionId')

  if [[ -z "$OUTSTANDING" ]]; then
    echo "no outstanding — checking state…"
    STATUS=$(goctl --env dev captain --api-version v2 journey-state-fetch \
      --var instanceId="$INSTANCE" | jq -r .status)
    echo "journey status: $STATUS"
    [[ "$STATUS" == "Completed" ]] && break
    sleep 2; continue
  fi

  echo "outstanding: $OUTSTANDING (interaction: $INTID)"
  case "$OUTSTANDING" in
    *PrimaryDocument/side1Image*)
      jq -n --rawfile img "$DOC_B64" '{subject:{documents:[{type:"primary",side1Image:($img|rtrimstr("\n"))}]}}' > /tmp/ctx.json
      jq -n '[{domainElementId:"PrimaryDocument/side1Image"}]' > /tmp/parts.json
      ;;
    *Selfie/selfieImage*)
      # IMPORTANT: send ONLY {selfieImage: <b64>} — no `type`, no `anchorImage`.
      # The Document Extraction module that ran in segment1 wrote the anchor
      # as a SEPARATE biometrics[] entry already. Bundling anchorImage with
      # selfie clobbers it.
      jq -n --rawfile face "$FACE_B64" \
        '{subject:{biometrics:[{
          selfieImage:($face|rtrimstr("\n"))
        }]}}' > /tmp/ctx.json
      jq -n '[{domainElementId:"Selfie/selfieImage"}]' > /tmp/parts.json
      ;;
    *)
      echo "Unknown outstanding ref: $OUTSTANDING — add a handler" >&2; exit 1
      ;;
  esac

  goctl --env dev captain --api-version v2 interaction-submit \
    --var instanceId="$INSTANCE" \
    --var interactionId="$INTID" \
    --var context=@/tmp/ctx.json \
    --var participants=@/tmp/parts.json \
    --token "$END_USER_JWT" > /dev/null
done

echo "✓ done"
```

Save this somewhere, fill in the `RESOURCE` value, run it, watch a journey complete.

---

## Pitfalls and how we hit them

| Symptom | Cause | Fix |
|---|---|---|
| `journey-start` v2 → 400 `context.config: ... received undefined` | v2 requires `context.config.delivery` | `--var context='{"config":{"delivery":"api"}}'` |
| `journey-start` v1 → 400 `context.config: …` | v1 manifest was stale (old `{journeyId, externalId}` shape) | Manifest patched to current `{resourceId, context}` shape |
| `device-start` → 401 `Authentication required` | Manifest path `/device/start` is wrong; current upstream mount is `/journey/device/start` | Manifest patched |
| `device-start` → 400 `scope: expected array, received string` | Manifest had `scope` as string with empty-string default | Manifest changed `scope` to `kind: json` with default `"[]"` |
| `device-connect` → 401 | Connect token must go in `Authorization: Bearer` header, not body (the legacy body form is deprecated) | Curl-bypass for now; CLI feature pending |
| `interaction-fetch` → 401 | Customer token used; this endpoint needs the end-user token | Use `--token <endUserToken>` from `device-connect` |
| `interaction-submit` accepted (`status: success`) but `outstanding` doesn't change | Submit was missing `participants[]` OR was using wrong discriminator | Send BOTH `context.subject.<plural>` with the right `type: "<discriminator>"` AND `participants[]` with the matching `domainElementId` |
| `interaction-submit` → 400 `context.subject: expected object, received undefined` | Submitted with only `participants` and no `context` | Both are required together |
| Journey reaches `Completed` but Facematch shows "System Error" | Selfie submit bundled `anchorImage` with selfie (or worse, set a `type:` field) — that overwrites the anchor entry that Document Extraction had populated | Submit ONLY `{selfieImage: <b64>}` under `biometrics[]`. Don't add a `type` field; don't add `anchorImage`. The anchor is a SEPARATE biometrics entry, written by the extraction module from the document. |
| `--var context='@file.json'` doesn't substitute the base64 | Used inline shell expansion with multi-MB content (argument list too long) | Use `jq -n --rawfile img <path> '{…}'` to build the file, then `--var context=@<file>` |

---

## Reference: the data-path table

This is the cheat sheet for translating an `outstanding` ref into a submit payload. Each row: the ref namespace from `outstanding`, the matching discriminator in `context.subject`, and a minimal example body.

```
PrimaryDocument/<field>   →   subject.documents[ {type: "primary",   <field>: <value>} ]
SecondaryDocument/<field> →   subject.documents[ {type: "secondary", <field>: <value>} ]
Selfie/<field>            →   subject.biometrics[ {<field>: <value>} ]   ← NO type discriminator; anchor lives in a sibling entry populated by extraction
Subject/identity          →   subject.identity = { firstName, middleNames[], lastNames[], dateOfBirth, ... }
```

For `Selfie/selfieImage` specifically: send ONLY `{selfieImage: <b64>}`. Do NOT include a `type:` field, do NOT bundle `anchorImage`. The Document Extraction module that runs after segment1 writes the document-face into a SEPARATE biometrics entry as the anchor — your submit should not clobber it.

Always pair with `participants: [{domainElementId: "<the ref>"}]`.

The authoritative source for the discriminator mappings is the journey interpreter (`identity-gbggo-platform/packages/interpreter/src/utils/`), specifically the `createDocumentEntry` / similar entries that bind a slot id (`PrimaryDocument`, `Selfie`, …) to a discriminator field+value pair. When you encounter a new ref the table above doesn't cover, grep the interpreter source for the slot id.

---

## What's not yet automated (open follow-ups)

1. **`device-connect` via the CLI directly.** Today the connect-secret goes in the `Authorization` header, but `goctl --token` is JWT-shape-validating. Until we add a raw-bearer mode (or a new `auth: connect-secret` taxonomy entry), use the curl one-liner from Step 2.
2. **A `--submit-element <ref>=@<file>` helper.** The two-part `context` + `participants` dance is mechanical; a single flag could build both halves from a `ref=value` pair. Not yet built.
3. **The full discriminator catalogue.** We've verified `PrimaryDocument` (`type: "primary"`) and `Selfie` (`type: "selfie"`). Other slots exist (`SecondaryDocument`, document-type variants like `Passport`/`DriversLicense`, etc.) — discover them as new journeys surface them.
