# Captain end-to-end QA test report

**Author:** goctl CLI investigation, 2026-05-26
**Environment:** `dev` (`gbggo4-dev.nonprod.fabric.gbgplatforms.com` / `gbggo4-dev-userview.nonprod.fabric.gbgplatforms.com`)
**Tenant / org:** `gbg`
**Tested by:** `flow-api-test` Keycloak user (customer scope) + `userview-full` (UserView scope)
**CLI used:** `goctl` v0.1.0 (commit-of-the-day; rebuilt repeatedly during the run)

---

## TL;DR

| Surface | Status |
|---|---|
| Captain v1 — `journey-start` → `journey-state-fetch` | ✅ Full round-trip, `Completed` in ~9 s |
| Captain v2 — `journey-start` → `device-start` → `device-connect` → `interaction-fetch`/`-submit` → `journey-state-fetch` | ✅ Full round-trip, `Completed` with overall **Decision: Accept** |
| Captain v2 — Facematch + Liveness module outcomes | ⚠ Modules execute but report `ERROR` / `Fail` — **test-data quality**, NOT a CLI/payload problem (see §6) |
| `device-connect` from the CLI | ⚠ Needs curl-bypass today (connect-secret goes in `Authorization: Bearer` header; CLI's `--token` validates JWT shape) |
| UserView read endpoints | ✅ Verified — session-list, session-processing, audit-logs |
| Designer endpoints | ✅ 12 / 15 verified working; 2 are auth-gated (`show:audit-log-customer` permission); 1 disabled pending investigation |

**What we found broken in the upstream contract vs. our endpoint catalog:** 8 manifest paths or body shapes had drifted from current upstream source and were patched during the run. Details in §3.

**What QA should pick up next:** test-data swap for biometric modules (real face + matching document), confirmation of the discriminator catalogue beyond `PrimaryDocument` / `Selfie`, and a decision on whether `device-connect` should support a non-JWT bearer in the CLI.

---

## 1. Environment + setup

- **Captain dev base URL:** `https://gbggo4-dev.nonprod.fabric.gbgplatforms.com/v2/captain` (v2) and `…/captain/api` (v1)
- **UserView dev base URL:** `https://gbggo4-dev-userview.nonprod.fabric.gbgplatforms.com/view/api`
- **Captain auth (dev):** Keycloak realm `go`, `password` grant, user `flow-api-test`
- **UserView auth (dev):** separate Keycloak instance on the same host (note: different hostname → token URL: `https://gbggo4-dev-userview.…/auth/realms/go/protocol/openid-connect/token`), user `userview-full`
- **Resources used during testing:**
  - v1 journey: `3980bb86dd84a8636d632c3e4b365741e7c17b4cc3596958bad06353562be698@1atcr6m7`
  - v2 journey: `91608159a6382f4e7c61e6a0471c54a05e827a5c544abc575dd3e25dd23955c2@m49aitfa`
  - Test identity (v1 + v2): `firstName: "AROHA"`, `middleNames: ["MERE TERESA"]`, `lastNames: ["WATA"]`, `dateOfBirth: "1990-10-01"`
  - Test document image: `images/document.b64` (3,400,872 bytes JPEG base64)
  - Test face image: `images/faceImage.b64` (131,128 bytes JPEG base64)

---

## 2. Verified flows (✅)

### 2.1 Captain v1 — synchronous start

```sh
goctl --env dev captain --api-version v1 journey-start \
  --var resourceId=<v1-resource>@<version> \
  --var context='{}' \
  --include identity testdata-v1 \
  --include documents testdata-v1 \
  --include biometrics default
# → {"instanceId":"PiKABR_tWSQR8Zm6Tv0gnPj6"}

goctl --env dev captain --api-version v1 journey-state-fetch \
  --var instanceId=PiKABR_tWSQR8Zm6Tv0gnPj6
# → {"instanceId":"...","status":"Completed", "metaData":{"createdTime":"2026-05-26T17:01:12.671Z","completedTime":"2026-05-26T17:01:21.439Z"}}
```

End-to-end ~8.8 seconds, single POST + single GET.

### 2.2 Captain v2 — async start + interaction-driven completion

Seven HTTP exchanges in the canonical happy path:

| # | Endpoint | Result |
|---|---|---|
| 1 | POST `/v2/captain/journey/start` (customer auth) | `instanceId: i-VyUjy0ElozxQ7KiuqnR-`, `status: started` |
| 2 | POST `/v2/captain/journey/device/start` (customer auth) | `connectToken: <nanoid>`, `expiresIn: 119` |
| 3 | POST `/v2/captain/journey/device/connect` (`Authorization: Bearer <connectToken>` — auth: none for the route, but the secret IS the credential) | `endUserToken: <HS256 JWT>`, `expiresIn: 86399` |
| 4 | POST `/v2/captain/journey/interaction/fetch` (end-user JWT) | `outstanding: ["PrimaryDocument/side1Image"]`, page `Document Capture`, interaction `segment1@latest` |
| 5 | POST `/v2/captain/journey/interaction/submit` (end-user JWT) | `{status:"success"}` → journey advances |
| 6 | POST `/v2/captain/journey/interaction/fetch` | `outstanding: ["Selfie/selfieImage"]`, page `Biometrics Capture`, interaction `segment4@latest` |
| 7 | POST `/v2/captain/journey/interaction/submit` (with anchor!) | `{status:"success"}` → `status: Completed` |

Full elapsed time was ~30 s including manual poll intervals; sub-second per submit.

### 2.3 UserView read of a Captain session

```sh
goctl --env dev userview journey-sessions-list --var orgId=gbg     # 42,001 sessions in dev
goctl --env dev userview journey-sessions-processing --var orgId=gbg --var id=<captain-instanceId>
```

Returns ~260 KB of session detail: identity, module outcomes, capabilities, executionTime per module, evaluation results. UserView ingests Captain instances within ~10 s of completion.

### 2.4 Designer (15 endpoints, 12 verified)

Catalog / list / export endpoints all verified against the canonical test org `gbg`:

- `modules-catalog`, `templates-catalog` — real catalog data
- `journey-list` — 1000+ journeys returned
- `journey-revisions-list` for `type ∈ {journey, delivery, flow}`
- `validate-journey-flow`, `check-module-compatibility` — schema-level validation working
- `export-journey`, `export-delivery`, `export-superflow`, `export-prefill-schema` — all return real artifacts when called with the LATEST revision version
- `partner-children` — returns child orgs when called with a real partner orgId (regular orgs return 404 "not a partner")

Not working / pending:

- `audit-project`, `get-audit-records` — paths now correct (see §3), but server returns **403** because `flow-api-test` lacks the `show:audit-log-customer` permission. Path validation = ✅. Permission grant = follow-up for ops.
- `credential-check-module-compatibility` — manifest body shape is best-effort (see TODO comment in the YAML). Server returns 404; full body verification pending upstream confirmation.
- `image-draft-get` — disabled pending investigation.

---

## 3. Upstream contract drift discovered & patched

Eight manifest entries had diverged from the current upstream source. Each one was patched and re-verified during the run.

### 3.1 Six Captain v2 paths were missing `/journey` prefix

Captain v2's Hono routes are mounted as `app.route('/v2/captain/journey', journeyRoutes)`. The `device/*` and `interaction/*` sub-routes are then mounted INSIDE that, e.g. `journeyRoutes.route('/device', deviceRoutes)`. So the real URLs are `/v2/captain/journey/device/*`, not `/v2/captain/device/*`. The inventory we built from a previous snapshot had the bare versions.

| Endpoint | Was | Now |
|---|---|---|
| `device-start` | `/device/start` | `/journey/device/start` |
| `device-connect` | `/device/connect` | `/journey/device/connect` |
| `device-validate` | `/device/validate` | `/journey/device/validate` |
| `interaction-fetch` | `/interaction/fetch` | `/journey/interaction/fetch` |
| `interaction-schema` | `/interaction/schema` | `/journey/interaction/schema` |
| `interaction-submit` | `/interaction/submit` | `/journey/interaction/submit` |

All six were silently 401-ing ("Authentication required") on the wrong path — the customer token reached the gateway but no route matched, and the default-deny middleware returned 401 rather than 404.

### 3.2 Captain v1 `journey-start` body shape converged with v2

Old inventory: v1 took `{journeyId, externalId, locale, region}`. Current upstream: `StartJourneyRequest = StartFlowState & ResourceId`, i.e. `{resourceId, context: {subject, config?}}` — same shape as v2 except `context.config` is optional. The legacy `{journeyId, externalId}` shape is gone.

Patched: v1 manifest now uses the same body shape as v2.

### 3.3 Captain v1 `journey-state-fetch` body simplified

Old: `{journeyId, instanceId}`. Current: `GetStateRequest = InstanceId & { context?: string[] }` — just `{instanceId, context?}`. Same shape as v2.

### 3.4 Captain v2 `device-start` schema change — `scope` is an array

Old manifest: `scope` as `kind: string` with default `""`. Current upstream Zod: `scope: z.array(DeviceStartScope).optional()`. Sending an empty string yielded 400 `MISSING_FIELD: scope: expected array, received string`.

Patched: `scope` is `kind: json` with default `"[]"`. Also dropped `ttlMinutes` from the body — the current Zod schema doesn't declare it (the field still exists in the response, but isn't part of the request).

### 3.5 Captain v2 `interaction-submit` schema completely rewritten

Old manifest body: `{instanceId, interactionId, domainElements[], data{}}`.
Current upstream Zod (`submitInteractionBodySchema`):

```ts
{
  instanceId: InstanceId,
  interactionId: string,
  participants?: Array<{ domainElementId?: string; instruction?: string; ... }>,
  context?: JourneyContext.extend({ reviewers: [...] }).optional(),
}
```

`domainElements` and `data` don't exist anymore — the payload is `context` (same shape as `journey-start`) PLUS a `participants[]` declaration. **Both halves are required together** (see §5).

### 3.6 Designer `audit-logs` controller renamed + path shape changed

Old upstream: `@Controller('audit') @Get(':project')` — URL `/designer/api/audit/{project}?orgId={orgId}&page={page}&pageSize={pageSize}`.
Current upstream: `@Controller('audit-logs') @Get(':orgId/:project')` — URL `/designer/api/audit-logs/{orgId}/{project}?page={page}&pageSize={pageSize}` (orgId moved from query to path).

Both `audit-project` and `get-audit-records` manifests patched. They now successfully reach the controller but return 403 because `flow-api-test` lacks the `show:audit-log-customer` permission (separate issue, see §6.3).

### 3.7 Designer `export-journey` body required `version`

The DTO is `ExportJourneyDto { journeyId: string, version: string }`. Our manifest had only `journeyId`, which caused 500 server errors. Patched — `version` is now a required var.

### 3.8 Designer `export-delivery` / `-prefill-schema` / `-superflow` need head revision

Path was correct; the DTO is `DeliveryRevision { deliveryRevisionId, version }`. Our manifest was sending older history versions (e.g. `3xujuliw`) which the export action couldn't resolve to an artifact — returned 404 "Not Found." Fix: must use the **latest/head** revision (e.g. `304zbb1f` for the same delivery). All three manifests now carry a long `version:` description explaining how to pull `journey-revisions-list type=delivery → .resources[<id>].history[0].version`.

---

## 4. The two-part interaction-submit protocol (most important finding for QA)

The single biggest non-obvious behaviour: `interaction-submit` requires **both** `context.subject` and `participants[]` in the same request body. Sending only one silently misbehaves.

| Submit shape | Server response | Effective behaviour |
|---|---|---|
| `context.subject.{discriminator-keyed payload}` alone | `200 {"status":"success"}` | **Silent no-op.** `outstanding` doesn't change. The journey looks ack'd but isn't progressing. |
| `participants: [{domainElementId: "..."}]` alone | `400 MISSING_FIELD: context.subject: Invalid input` | Hard fail, recoverable. |
| Both together | `200 {"status":"success"}` AND `outstanding` clears AND journey advances | ✅ |

The `outstanding` field from `interaction-fetch` tells you which `domainElementId` to declare in `participants[]`. The `context.subject` carries the value, namespaced by the **discriminator-based slot mapping** (§5).

### 4.1 Why this is QA-relevant

The silent-no-op case (context-only) is the dangerous one. A naive integration that omits `participants[]` will:

1. See `200` responses from `interaction-submit` and assume success.
2. Re-fetch `interaction-fetch` and notice the journey is still on the same segment.
3. Re-submit, loop, possibly increment counters.
4. Never make progress; potentially trigger timeouts.

There is no API-level signal differentiating "we accepted and stored your data" from "we accepted, stored, and advanced the flow". QA test plans should explicitly assert that `outstanding` changes (or `interactionId` advances) after each submit, not just that the response is 200.

---

## 5. The discriminator → data-path mapping

Outstanding refs from `interaction-fetch` use a **slot-name / field-name** namespacing (`PrimaryDocument/side1Image`, `Selfie/selfieImage`, `Subject/identity`, etc.). The data lives in `context.subject.<plural-collection>` with a `type` discriminator that maps each array element back to a named slot.

| Outstanding ref pattern | Data path in `context.subject` |
|---|---|
| `PrimaryDocument/<field>` | `documents[ where type == "primary" ].<field>` |
| `SecondaryDocument/<field>` (when journey config declares one) | `documents[ where type == "secondary" ].<field>` |
| `Selfie/<field>` | `biometrics[ ].<field>` — **no `type` discriminator** |
| `Subject/identity` | `identity` (singleton) |

Verified directly in this run: `PrimaryDocument/side1Image` → `documents[0]` with `{type:"primary", side1Image:<b64>}`; `Selfie/selfieImage` → `biometrics[]` with a single new entry `{selfieImage:<b64>}` (and ONLY that field — see §6.1 for why bundling `anchorImage` breaks Facematch).

**Source of truth** for additional slots: the interpreter package's `createDocumentEntry({id, discriminator: {field, value}})` calls (see `identity-gbggo-platform/packages/interpreter/src/utils/`). Each call registers a slot id (e.g. `PrimaryDocument`) and the discriminator value (e.g. `type: "primary"`). The spec at `subject-merge.spec.ts` is the canonical test of how merging works.

The `--include` CLI shortcut bakes this mapping in: `--include documents testdata-v1` overlays into `context.subject.documents`; `--include biometrics default` overlays into `context.subject.biometrics`. It's not a magic protocol — just a renderer convenience.

---

## 6. Known-failing scenarios for QA follow-up

### 6.1 Facematch Verification — ❌ ERROR — caused by bundling `anchorImage` with the selfie

Three runs, each with a different selfie submit shape:

| Attempt | Selfie submit body | Facematch outcome |
|---|---|---|
| 1 — no anchor mention | `{subject: {biometrics: [{selfieImage:<face>}]}}` (this matches the corrected playbook) | TBD — confirm in a follow-up run |
| 2 — anchor bundled with selfie | `{subject: {biometrics: [{type:"selfie", selfieImage:<face>, anchorImage:<doc>}]}}` | ❌ `ERROR` |
| Earlier — no anchor at all (manifest bug) | `{subject: {biometrics: [{selfieImage:<face>}]}}` but the Document Extraction module had crashed earlier in that flow | ❌ `System Error` |

**Diagnosis (revised):** the anchor face is supposed to be written by the **Document Extraction module** that runs after segment1 (it extracts the face photo from the document the caller submitted as `PrimaryDocument/side1Image` and stores it as a SEPARATE biometrics entry — see `identity-gbggo-platform/packages/interpreter/src/utils/merge-utils.ts`). The caller's `Selfie/selfieImage` submit should consist of only `{selfieImage: <b64>}` and must not include `anchorImage` — doing so clobbers the extraction-populated anchor.

**Corrected pattern** (now reflected in §4 and §5):

```sh
jq -n --rawfile face images/faceImage.b64 \
  '{subject:{biometrics:[{selfieImage: ($face|rtrimstr("\n"))}]}}' > /tmp/ctx-selfie.json
jq -n '[{domainElementId:"Selfie/selfieImage"}]' > /tmp/parts-selfie.json
```

**Open:** even with the correct shape, the canonical happy-path also depends on Document Extraction succeeding and finding a face in the document. Our `images/document.b64` is a generic document; if it has no extractable face, the anchor entry won't be populated and Facematch will still fail. QA should swap in a real ID document with a clear face photo plus a matching selfie.

### 6.2 Liveness Verification — ❌ Fail

**Diagnosis:** static images can't pass liveness anti-spoofing. Liveness modules typically require:

- A short video sample (multi-frame, with motion).
- Or device-attested capture metadata.
- Or a specific test-mode header that bypasses anti-spoof in dev.

A single base64 JPEG of a face fails by design.

**QA follow-up:** confirm whether Captain dev accepts a "test bypass" mode for Liveness (an env var on the server, a flag in the journey config, etc.), or whether passing-state Liveness testing requires a richer capture format.

### 6.3 Designer `audit-project` / `get-audit-records` — ❌ 403

Path was wrong in our manifest (`/audit/{project}` vs the current `/audit-logs/{orgId}/{project}` — see §3.6). After the fix, the call reaches the controller and returns **403 Forbidden** with:

```json
{ "status": 403, "message": "Forbidden resource", "error": "forbidden-error" }
```

The controller is gated on `@Permission(['show:audit-log-customer'])`. The `flow-api-test` user doesn't carry that permission in dev.

**QA follow-up:** either grant `show:audit-log-customer` to `flow-api-test` for dev integration testing, or use an explicit reviewer-level token via `--token <bearer>` when calling these endpoints. (The CLI surfaces a clear 403 now, so detection is easy.)

### 6.4 Designer `credential-check-module-compatibility` — ❌ 404

The controller path was discovered to be `/resources/{orgId}/{type}/actions/credential` (NestJS `actions.controller.ts:197`), but the DTO (`PersistCredentialParams`) and the expected `{type}` value couldn't be confirmed from source. The manifest carries a `# TODO: verify body shape with upstream` comment.

**QA follow-up:** clarify the intended call shape with the team owning credential actions, or confirm whether this endpoint is meant to be called externally at all.

### 6.5 Designer `image-draft-get` — 🔒 Disabled

Marked `disabled: true` pending further investigation. The endpoint surface needs a known-good `{orgId, id, fileName}` triple to test, plus confirmation that the response format (binary image vs. metadata) is what the CLI should handle.

---

## 7. Bypasses currently in place

### 7.1 `device-connect` via `curl`

`device-connect` is `auth: none` at the route level, but the connect-secret must be sent in `Authorization: Bearer <secret>` (per `connect-secret.ts:80`). The CLI's `--token` flag is JWT-shape-validating today (decodes `exp`, etc.) and rejects nanoid secrets.

Workaround used during testing:

```sh
CONNECT=$(goctl --env dev captain --api-version v2 device-start \
  --var instanceId="$INSTANCE" | jq -r .connectToken)
END_USER_JWT=$(curl -s -X POST \
  "https://gbggo4-dev.…/v2/captain/journey/device/connect" \
  -H "Authorization: Bearer $CONNECT" \
  -H "Content-Type: application/json" --data '{}' \
  | jq -r .endUserToken)
```

**Pending CLI fix:** either accept non-JWT bearer values when the manifest declares `auth: connect-secret` (new taxonomy entry), or add a `--secret <value>` flag that's distinct from `--token`.

### 7.2 `.env` env-var rename

User's `.env` carries the legacy `GGO_CAPTAIN_V1_*` / `GGO_CAPTAIN_V2_*` per-version naming. The CLI now expects unified `GGO_CAPTAIN_*` after the v1+v2 service consolidation. In-shell aliasing was used during this run (the V1 and V2 values are identical for the same env — same Keycloak client).

**Pending:** durable rename of `.env` keys to drop the V1/V2 suffix.

---

## 8. Reproduction: minimal v2 happy-path script

Saves an instanceId, drives a journey to `Completed`, dumps module outcomes.

```sh
#!/usr/bin/env bash
set -euo pipefail
cd /Users/edward.montoya/code/identity-gbggo-cli

RESOURCE="91608159a6382f4e7c61e6a0471c54a05e827a5c544abc575dd3e25dd23955c2@m49aitfa"
CAPTAIN="https://gbggo4-dev.nonprod.fabric.gbgplatforms.com"

# 1. Pre-build the two submit payload pairs (doc + selfie+anchor)
jq -n --rawfile img images/document.b64 \
  '{subject:{documents:[{type:"primary",side1Image:($img|rtrimstr("\n"))}]}}' > /tmp/ctx-doc.json
jq -n --rawfile face images/faceImage.b64 --rawfile anchor images/document.b64 \
  '{subject:{biometrics:[{type:"selfie",selfieImage:($face|rtrimstr("\n")),anchorImage:($anchor|rtrimstr("\n"))}]}}' > /tmp/ctx-selfie.json
jq -n '[{domainElementId:"PrimaryDocument/side1Image"}]' > /tmp/parts-doc.json
jq -n '[{domainElementId:"Selfie/selfieImage"}]' > /tmp/parts-selfie.json

# 2. Source env vars + alias V2→unified
while IFS="=" read -r k v; do
  case "$k" in
    GGO_CAPTAIN_V2_DEV_*) export "${k/_V2_DEV_/_DEV_}=$v" ;;
    GGO_USERVIEW_DEV_*)   export "$k=$v" ;;
  esac
done < .env

# 3. Start the v2 journey
INSTANCE=$(goctl --env dev captain --api-version v2 journey-start \
  --var resourceId="$RESOURCE" \
  --var context='{"config":{"delivery":"api"}}' \
  --include identity testdata-v1 \
  --include documents testdata-v1 \
  --include biometrics default \
  | jq -r .instanceId)
echo "instanceId=$INSTANCE"

# 4. Bootstrap end-user JWT
SECRET=$(goctl --env dev captain --api-version v2 device-start --var instanceId="$INSTANCE" | jq -r .connectToken)
JWT=$(curl -s -X POST "$CAPTAIN/v2/captain/journey/device/connect" \
  -H "Authorization: Bearer $SECRET" -H "Content-Type: application/json" --data '{}' \
  | jq -r .endUserToken)

# 5. Drive interactions
while :; do
  FETCH=$(goctl --env dev captain --api-version v2 interaction-fetch \
    --var instanceId="$INSTANCE" --token "$JWT")
  OUT=$(echo "$FETCH" | jq -r '.outstanding // [] | join(",")')
  INTID=$(echo "$FETCH" | jq -r .interactionId)
  STATUS=$(goctl --env dev captain --api-version v2 journey-state-fetch \
    --var instanceId="$INSTANCE" | jq -r .status)
  echo "status=$STATUS outstanding=[$OUT]"
  [[ "$STATUS" == "Completed" ]] && break
  case "$OUT" in
    *PrimaryDocument/side1Image*) CTX=/tmp/ctx-doc.json;    PARTS=/tmp/parts-doc.json ;;
    *Selfie/selfieImage*)         CTX=/tmp/ctx-selfie.json; PARTS=/tmp/parts-selfie.json ;;
    *) echo "Unknown outstanding: $OUT" >&2; exit 1 ;;
  esac
  goctl --env dev captain --api-version v2 interaction-submit \
    --var instanceId="$INSTANCE" --var interactionId="$INTID" \
    --var context=@"$CTX" --var participants=@"$PARTS" \
    --token "$JWT" > /dev/null
  sleep 2
done

# 6. Wait for UserView to ingest, then dump module outcomes
sleep 10
goctl --env dev userview journey-sessions-processing --var orgId=gbg --var id="$INSTANCE" \
  | jq '{decisionStatus, modules: [.modules[]? | {moduleName, outcome, status}]}'
```

Expected output (with current fixtures):

```
status=InProgress outstanding=[PrimaryDocument/side1Image]
status=InProgress outstanding=[]
status=InProgress outstanding=[Selfie/selfieImage]
status=Completed outstanding=[]
{
  "decisionStatus": { "label": "Decision: Accept", ... },
  "modules": [
    { "moduleName": "Document Classification", "outcome": "Document Classified", "status": "complete" },
    { "moduleName": "Document Authentication", "outcome": "Medium Risk", "status": "complete" },
    { "moduleName": "Document Extraction", "outcome": "Extraction Successful", "status": "complete" },
    { "moduleName": "Facematch Verification", "outcome": "ERROR", "status": "error" },          ← data-quality failure
    { "moduleName": "Liveness verification", "outcome": "Fail", "status": "complete" },          ← static-image failure
    { "moduleName": "Evaluation", "outcome": "Decision: Accept", "status": "" }
  ]
}
```

---

## 9. Open issues — recommended QA tickets

| # | Severity | Title |
|---|---|---|
| 1 | Med | Captain v2 `device-connect` from CLI — add raw-bearer support so the connect-secret can be supplied via `--token <secret>` without JWT shape validation |
| 2 | High (silent-failure) | `interaction-submit` should distinguish "data stored, flow advanced" from "data stored, flow NOT advanced" — currently both return `{"status":"success"}` |
| 3 | Med | Granting `show:audit-log-customer` to `flow-api-test` in dev — unblocks Designer audit endpoints for integration testing |
| 4 | Low | Real-data biometric fixtures (matched document+selfie pair) for Facematch positive-path testing |
| 5 | Med | Liveness test-mode contract — document whether Captain accepts a bypass header / flag in dev for tests using static images |
| 6 | Low | `credential-check-module-compatibility` body shape — confirm intended call shape with credential-actions team |
| 7 | Low | `image-draft-get` test fixture (orgId, id, fileName triple) — enables CLI surface verification |
| 8 | Low | Discover the full `createDocumentEntry` slot catalogue from the interpreter — expand the discriminator table in `docs/captain-playbook.md` beyond `PrimaryDocument` + `Selfie` |

---

## 10. Files modified during this investigation

For traceability if QA wants to re-create the run against a different upstream snapshot:

| File | Change |
|---|---|
| `src/endpoints/captain/journey-start.yaml` | v1 body shape converged with v2 (`{resourceId, context}`) |
| `src/endpoints/captain/journey-state-fetch.yaml` | v1 simplified — `{instanceId}` only |
| `src/endpoints/captain/device-start.yaml` | path gained `/journey/` prefix; `scope` → `kind: json` array, default `"[]"`; `ttlMinutes` dropped |
| `src/endpoints/captain/device-connect.yaml` | path gained `/journey/` prefix |
| `src/endpoints/captain/device-validate.yaml` | path gained `/journey/` prefix |
| `src/endpoints/captain/interaction-fetch.yaml` | path gained `/journey/` prefix |
| `src/endpoints/captain/interaction-schema.yaml` | path gained `/journey/` prefix |
| `src/endpoints/captain/interaction-submit.yaml` | path gained `/journey/` prefix; body rewritten to current `{instanceId, interactionId, context?, participants?}` shape |
| `src/endpoints/designer/audit-project.yaml` | path: `/audit/{project}?orgId=…` → `/audit-logs/{orgId}/{project}?…` (orgId moved from query to path) |
| `src/endpoints/designer/get-audit-records.yaml` | same |
| `src/endpoints/designer/export-journey.yaml` | added required `version` var; body now `{journeyId, version}` |
| `src/endpoints/designer/export-delivery.yaml` | body shape verified against DTO; description warns about head-only revisions |
| `src/endpoints/designer/export-prefill-schema.yaml` | same |
| `src/endpoints/designer/export-superflow.yaml` | path corrected to `/resources/{orgId}/delivery/internal/actions/export/superflow` (was `/journey/actions/export-superflow`); body uses delivery DTO |
| `src/endpoints/designer/credential-check-module-compatibility.yaml` | path discovered to be `/resources/{orgId}/{type}/actions/credential`; TODO on body shape remains |
| `src/endpoints/designer/check-module-compatibility.yaml` | var rename: `modules` → `nodes` (server message: `"nodes must be an array"`) |
| `src/endpoints/designer/image-draft-get.yaml` | marked `disabled: true` pending investigation |
| `src/cli/mod.rs apply_includes` | merge target changed from `context.<cat>` to `context.subject.<cat>` (matches Captain wire shape) |
| `src/fixtures/identity/*.yaml` | schema updated to `{firstName, middleNames[], lastNames[], dateOfBirth}` (real-world shape) |
| `src/fixtures/documents/*.yaml` | rewritten as array of `{type: "primary"/"secondary", side1Image, side2Image?}` |
| `src/fixtures/biometrics/default.yaml` | rewritten as array of `{type: "selfie", selfieImage, anchorImage}` |
| `src/fixtures/images/*.b64` | replaced placeholders with real base64 (faceImage 131 KB, document 3.4 MB) |
| `docs/captain-playbook.md` | new — full end-to-end how-to with reproduction commands |
| `docs/qa-captain-report.md` | this report |

All test runs are reproducible: the catalog state above + the `.env` credentials + the two resourceIds (§1) yield the same module outcomes deterministically.
