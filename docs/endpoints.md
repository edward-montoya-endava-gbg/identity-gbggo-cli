# goctl — endpoint catalog

This file is generated from the embedded endpoint catalog. To refresh:
`goctl list-endpoints --json | <regenerator script>`. Or run
`goctl describe <service> <command>` for the full manifest of any one endpoint.

**Catalog size:** 57 endpoints across 3 services
(captain: 17 versioned · designer: 27 · userview: 13). 17 designer/userview
endpoints are disabled-by-default. All Captain endpoints are enabled (per
version).

Disabled endpoints refuse to run — invoking one exits `Usage (2)` immediately,
before any auth, var resolution, or `--dry-run` short-circuit. For flat
(non-versioned) services edit `src/endpoints/<service>/<name>.yaml` and remove
the `disabled: true` / `disabled_reason:` lines. For Captain the gate is
per-version: remove `disabled: true` from the relevant `versions.<v>:` block.
Then run `cargo install --path . --force`.

## `captain` (17 endpoints — versioned)

Each row lists the wire details for **v2** (or, for v1-only endpoints, **v1**).
The `versions` column shows which versions this endpoint supports. Pick the
wire shape with `--api-version <v>` (or set `default_version:` in `regions.yaml`).

| name | versions | method | path (representative) | required_vars (representative) | status |
|---|---|---|---|---|---|
| `device-connect` | v1, v2 | POST | `/device/connect` (v2) · `/journey/device/connect` (v1) | v2: connectToken · v1: secret, deviceId | ✅ enabled |
| `device-start` | v1, v2 | POST | `/device/start` (v2) · `/journey/device/start` (v1) | v2: instanceId · v1: deviceType | ✅ enabled |
| `device-validate` | v1, v2 | POST | `/device/validate` (v2) · `/journey/device/validate` (v1) | v2: token · v1: credential, type | ✅ enabled |
| `interaction-fetch` | v2 | POST | `/interaction/fetch` | instanceId | ✅ enabled |
| `interaction-schema` | v2 | POST | `/interaction/schema` | instanceId | ✅ enabled |
| `interaction-submit` | v2 | POST | `/interaction/submit` | instanceId, interactionId, domainElements, data | ✅ enabled |
| `journey-history` | v2 | POST | `/journey/history` | instanceId | ✅ enabled |
| `journey-schema-fetch` | v2 | POST | `/journey/schema/fetch` | instanceId | ✅ enabled |
| `journey-start` | v1, v2 | POST | `/journey/start` | v2: resourceId, context · v1: journeyId, externalId | ✅ enabled |
| `journey-state-delete` | v1, v2 | POST | `/journey/state/delete` | v2: instanceId · v1: journeyId, instanceId | ✅ enabled |
| `journey-state-fetch` | v1, v2 | POST | `/journey/state/fetch` | v2: instanceId · v1: journeyId, instanceId | ✅ enabled |
| `journey-task-list` | v1 | POST | `/journey/task/list` | journeyId, instanceId | ✅ enabled |
| `journey-task-list-schema` | v1 | POST | `/journey/task/list/schema` | journeyId, instanceId | ✅ enabled |
| `journey-task-schema` | v1 | POST | `/journey/task/schema` | journeyId, taskId | ✅ enabled |
| `journey-task-update` | v1 | POST | `/journey/task/update` | journeyId, taskId, intent, data | ✅ enabled |
| `journey-terminate` | v2 | POST | `/journey/terminate` | instanceId | ✅ enabled |
| `version` | v2 | GET | `/version` | — | ✅ enabled |

## `designer` (27 endpoints, 13 disabled)

| name | method | path | required_vars | status |
|---|---|---|---|---|
| `add-license` | POST | `/resources/{orgId}/journey/actions/add-license` | orgId, journeyId, licenseType | 🔒 disabled (pending verification) |
| `archive-journey` | POST | `/resources/{orgId}/journey/actions/archive` | orgId, journeyId | 🔒 disabled (pending verification) |
| `audit-project` | GET | `/audit/{project}?orgId={orgId}&page={page}&pageSize={pageSize}&…` | project, orgId | ✅ enabled |
| `check-module-compatibility` | POST | `/resources/{orgId}/journey/actions/check-module-compatibility` | orgId, nodes | ✅ enabled |
| `create-interaction-journey` | POST | `/resources/{orgId}/journey/actions/create-interaction` | orgId, interactionData | 🔒 disabled (pending verification) |
| `create-journey` | POST | `/resources/{orgId}/journey/actions/create-journey` | orgId, flowData | 🔒 disabled (pending verification) |
| `create-journey-from-template` | POST | `/resources/{orgId}/{type}/actions/create-journey-from-template` | orgId, type, templateId, name | 🔒 disabled (pending verification) |
| `credential-check-module-compatibility` | POST | `/resources/{orgId}/{type}/actions/credential` | orgId, type, moduleGrId, credentialFormKey, credentialId, credential | ✅ enabled |
| `delivery-revert` | POST | `/resources/{orgId}/delivery/actions/delivery-revert` | orgId, deliveryId, revisionId | 🔒 disabled (pending verification) |
| `export-delivery` | POST | `/resources/{orgId}/delivery/internal/actions/export` | orgId, deliveryRevisionId, version | ✅ enabled |
| `export-journey` | POST | `/resources/{orgId}/journey/actions/export` | orgId, journeyId | ✅ enabled |
| `export-prefill-schema` | POST | `/resources/{orgId}/delivery/actions/schema/prefill/export` | orgId, deliveryRevisionId, version | ✅ enabled |
| `export-superflow` | POST | `/resources/{orgId}/delivery/internal/actions/export/superflow` | orgId, deliveryRevisionId, version | ✅ enabled |
| `get-audit-records` | GET | `/audit/{project}?orgId={orgId}&page={page}&pageSize={pageSize}&…` | project, orgId | ✅ enabled |
| `image-draft-get` | GET | `/{orgId}/image/draft/{id}/{fileName}` | orgId, id, fileName | 🔒 disabled (pending verification) |
| `journey-list` | POST | `/resources/{orgId}/journey/actions/list` | orgId | ✅ enabled |
| `journey-revisions-create` | POST | `/resources/{orgId}/{type}/revisions` | orgId, type, data, metadata | 🔒 disabled (pending verification) |
| `journey-revisions-list` | GET | `/resources/{orgId}/{type}/revisions` | orgId, type | ✅ enabled |
| `launch-flow` | POST | `/resources/{orgId}/delivery/actions/start/{region}` | orgId, deliveryId | 🔒 disabled (pending verification) |
| `link-journey-to-child` | POST | `/resources/{orgId}/journey/actions/link-journey-to-child` | orgId, parentId, childOrgId, childJourneyId | 🔒 disabled (pending verification) |
| `modules-catalog` | GET | `/modules/catalog` | — | ✅ enabled |
| `partner-children` | GET | `/partner/organisations/{orgId}/children` | orgId | ✅ enabled |
| `templates-catalog` | GET | `/templates/catalog` | — | ✅ enabled |
| `unarchive-journey` | POST | `/resources/{orgId}/journey/actions/unarchive` | orgId, journeyId | 🔒 disabled (pending verification) |
| `unlink-journey` | POST | `/resources/{orgId}/journey/actions/unlink-journey` | orgId, parentId, childId | 🔒 disabled (pending verification) |
| `update-org-settings` | POST | `/resources/{orgId}/organisation/actions/update-settings` | orgId, settings | 🔒 disabled (pending verification) |
| `validate-journey-flow` | POST | `/resources/{orgId}/journey/actions/validate-flow` | orgId, flow | ✅ enabled |

## `userview` (13 endpoints, 4 disabled)

| name | method | path | required_vars | status |
|---|---|---|---|---|
| `analytics` | GET | `/{orgId}/analytics` | orgId | ✅ enabled |
| `audit-logs-create` | POST | `/{orgId}/audit-logs` | orgId, sessionId, action | 🔒 disabled (pending verification) |
| `audit-logs-get` | GET | `/{orgId}/audit-logs/{sessionId}?page={page}&perPage={perPage}&sortBy={sortBy}&filterBy={filterBy}&mode={mode}` | orgId, sessionId | ✅ enabled |
| `audit-logs-list` | GET | `/{orgId}/audit-logs?page={page}&perPage={perPage}&sortBy={sortBy}&filterBy={filterBy}&mode={mode}` | orgId | ✅ enabled |
| `health` | GET | `/health` | — | ✅ enabled |
| `journey-sessions-change-decision` | POST | `/{orgId}/journey-sessions/processing/changeDecision` | orgId, journeySessionIds, decision, notes | 🔒 disabled (pending verification) |
| `journey-sessions-decision-status` | POST | `/{orgId}/journey-sessions/processing/decisionStatus` | orgId, journeySessionIds | ✅ enabled |
| `journey-sessions-delete` | DELETE | `/{orgId}/journey-sessions/{id}` | orgId, id | 🔒 disabled (pending verification) |
| `journey-sessions-export` | GET | `/{orgId}/journey-sessions/export` | orgId | ✅ enabled |
| `journey-sessions-list` | GET | `/{orgId}/journey-sessions?page={page}&perPage={perPage}&sortBy={sortBy}&filterBy={filterBy}&mode={mode}` | orgId | ✅ enabled |
| `journey-sessions-processing` | GET | `/{orgId}/journey-sessions/{id}/processing` | orgId, id | ✅ enabled |
| `preferences-ui-get` | GET | `/{orgId}/preferences/ui` | orgId | ✅ enabled |
| `preferences-ui-put` | PUT | `/{orgId}/preferences/ui` | orgId, preferences | 🔒 disabled (pending verification) |
