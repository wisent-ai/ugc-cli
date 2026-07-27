# ugc-cli

Private Rust CLI for provider-agnostic UGC campaign operations. It keeps the canonical state locally in SQLite and treats creator marketplaces as execution channels through capability-aware adapters.

## Scope

- provider connections and health checks
- campaigns and versioned briefs
- creator profiles and external identities
- assignments, shipping, and messaging
- versioned submissions and human review
- content-addressed asset ingestion with SHA-256
- ffprobe-backed technical QC
- usage-rights publication gates
- provider-owned or Echo-owned payments with idempotency
- durable outbox, retries, dead letters, polling, and signed webhooks
- immutable audit log and diagnostics

The CLI never stores plaintext provider secrets in SQLite. Connections store secret-source references. A source can be `env:NAME`, `file:/owner-only/path`, or `file:/owner-only/path#KEY`; the unprefixed legacy form still means an environment-variable name.

## Build and install

```bash
cargo build --release
cargo install --path .
```

The binary is named `ugc-cli`.

Optional runtime dependency: `ffprobe` enables video duration and dimensions during asset ingestion. Asset import still works without it and records a probe warning.

## Configuration

```bash
cp .env.example .env
export UGC_DB_PATH="$PWD/.ugc/ugc.db"
export UGC_ASSET_DIR="$PWD/.ugc/assets"
```

CLI-level `--db` and `--asset-dir` override environment defaults.

## Wisent tools

### Skarbiec: secret plane

Use Skarbiec for provider tokens, the Weles Supabase service credential, Brama request-signing credentials, webhook secrets, and recovery/audit policy. `ugc-cli` reads owner-only files produced by `skarbiec expand`; it never asks Skarbiec to print a value.

Create a template containing references only:

```bash
mkdir -p .ugc
cat > .ugc/wisent-tools.env.template <<'EOF'
WELES_SERVICE_ROLE_KEY=skarbiec://ugc-weles/service_role_key
BRAMA_REQUEST_SIGNING_SECRET=skarbiec://ugc-brama/request_signing_secret
UGC_HTTP_TOKEN=skarbiec://ugc-provider/api_token
UGC_HTTP_WEBHOOK_SECRET=skarbiec://ugc-provider/webhook_secret
EOF

skarbiec expand .ugc/wisent-tools.env.template \
  --out .ugc/wisent-tools.env
```

`skarbiec expand` writes the output with owner-only permissions. Check availability without printing a value:

```bash
ugc-cli skarbiec check \
  'file:.ugc/wisent-tools.env#WELES_SERVICE_ROLE_KEY'
```

Generate one reference line for a template:

```bash
ugc-cli skarbiec reference \
  --item ugc-provider \
  --field api_token \
  --name UGC_HTTP_TOKEN
```

Use Skarbiec when a secret must be shared, rotated, revoked, recovered, or audited. Do not commit the expanded env file; commit only its reference template.

### Weles: controlled browser execution

Use Weles only for an action backed by an existing trajectory and an authorized social account—for example account health, login recovery, marketplace/dashboard steps for which a reviewed trajectory exists, or Instagram publishing. Native provider APIs remain the first choice.

`ugc-cli` enqueues directly into Weles's canonical `account_action_logs` queue:

```bash
export WELES_SUPABASE_URL='https://PROJECT.supabase.co'
export WELES_TOKEN_SOURCE='file:.ugc/wisent-tools.env#WELES_SERVICE_ROLE_KEY'

ugc-cli weles enqueue \
  --account-id SOCIAL_ACCOUNT_UUID \
  --platform instagram \
  --action instagram_post \
  --params '{"svc_text":"Approved campaign caption"}'
```

The command returns the Weles queue row. Follow it without exposing credentials:

```bash
ugc-cli weles status WELES_JOB_ID
```

How it fits the UGC flow:

1. Echo/`ugc-cli` owns the campaign, approved asset, rights, and review decision.
2. The operator chooses an existing Weles trajectory and an authorized account.
3. `ugc-cli weles enqueue` records a queued browser action.
4. The Weles worker claims it, uses the account's isolated browser session, and writes terminal result/evidence to the same row.
5. The operator records the resulting publication URL in the canonical UGC workflow.

The current Weles `instagram_post` trajectory generates its own image. It does not publish an existing `ugc-cli` video asset. A dedicated, reviewed trajectory is required before claiming full UGC asset publication support. Never invent an action name: Weles silently skips actions absent from `src/worker/dispatch.ts`.

### Brama: model gateway

Use Brama for evidence-bounded assistance: brief critique, operational readiness, creator-fit review, rights-risk review, and submission-metadata triage. Brama selects a provider/model, handles ranked retries, and keeps provider credentials behind its Skarbiec-backed gateway.

Configure the signed client:

```bash
export BRAMA_URL='http://127.0.0.1:8080'
export UGC_BRAMA_AGENT_ID='ugc-operations'
export UGC_BRAMA_SIGNING_SECRET_SOURCE='file:.ugc/wisent-tools.env#BRAMA_REQUEST_SIGNING_SECRET'
```

Check the gateway:

```bash
ugc-cli brama health
```

Analyze any canonical record stored by `ugc-cli`:

```bash
ugc-cli brama analyze \
  --kind brief \
  --id BRIEF_ID \
  --model task:ugc-review \
  --instruction 'Find unsupported claims and missing required shots.'
```

Supported review contexts include `campaign`, `brief`, `creator`, `assignment`, `submission`, `asset`, and `usage_rights`. The complete canonical JSON is sent through Brama's OpenAI-compatible `/v1/chat/completions` endpoint with Brama's required HMAC headers. Model output is advisory: it cannot approve a submission, grant rights, initiate payment, or enqueue Weles by itself.

Recommended division of responsibility:

| Tool | Owns | Good UGC uses | Must not decide |
|---|---|---|---|
| Skarbiec | secrets, grants, encryption, audit | provider tokens, Weles credential, Brama signing key, webhook secrets | creative or campaign state |
| Weles | authorized browser trajectories and run evidence | reviewed dashboard actions, social-account health, supported publishing trajectories | rights, payment, final approval |
| Brama | authenticated model routing | brief critique, creator-fit and metadata-based risk review | legal rights, payment, autonomous publication |
| `ugc-cli` | canonical UGC state and gates | campaigns, briefs, assignments, assets, QC, review, rights, payments | provider credential storage |

## Provider modes

### Manual

A complete assisted-workflow adapter. Publishing creates a stable external reference and marks operations requiring work in the provider dashboard as `manual_action_required`.

```bash
ugc-cli connection add --name billo-manual --provider manual
```

### HTTP

A native adapter for a provider gateway implementing the contract below.

Create an owner-only credential file with Skarbiec as shown above, then reference the values:

```bash
ugc-cli connection add \
  --name primary-provider \
  --provider http \
  --base-url https://ugc-gateway.example.com \
  --token-source 'file:.ugc/wisent-tools.env#UGC_HTTP_TOKEN' \
  --webhook-secret-source 'file:.ugc/wisent-tools.env#UGC_HTTP_WEBHOOK_SECRET'
```

The adapter uses bearer authentication and these endpoints:

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | connection health |
| POST | `/campaigns` | publish canonical campaign payload |
| POST | `/messages` | deliver outbound message |
| POST | `/revisions` | request a submission revision |
| GET | `/events?cursor=...` | incremental reconciliation |

Campaign creation returns:

```json
{
  "external_campaign_id": "provider-campaign-id",
  "external_url": "https://provider.example/campaign/id",
  "status": "published"
}
```

Polling returns:

```json
{
  "events": [
    {
      "external_id": "event-id",
      "kind": "submission.created",
      "aggregate_type": "submission",
      "aggregate_external_id": "submission-id",
      "payload": {},
      "occurred_at": "2026-07-26T12:00:00Z"
    }
  ],
  "next_cursor": "opaque-cursor"
}
```

Webhook requests use the same event object. The sender signs the raw body with HMAC-SHA256 and sends the hexadecimal digest in `X-UGC-Signature` or `X-Signature`.

## End-to-end flow

### Create and approve a brief

```bash
campaign=$(ugc-cli campaign create \
  --name summer-launch \
  --brand Example \
  --product Serum \
  --objective conversions \
  --markets US \
  --languages en \
  --channels tiktok,reels)

campaign_id=$(printf '%s' "$campaign" | jq -r .id)

brief=$(ugc-cli brief add \
  --campaign "$campaign_id" \
  --service-type ugc_content \
  --creative-angle 'unexpected discovery' \
  --requirements 'show product,show result' \
  --forbidden-claims 'guaranteed cure' \
  --required-shots 'hook,product close-up,result' \
  --talking-points 'what changed,why it matters' \
  --aspect-ratios '9:16' \
  --raw-footage-required)

brief_id=$(printf '%s' "$brief" | jq -r .id)
ugc-cli brief approve "$brief_id"
```

### Add a creator and assignment

```bash
creator=$(ugc-cli creator add \
  --name 'Creator Name' \
  --email creator@example.com \
  --languages en \
  --markets US \
  --niches beauty)

creator_id=$(printf '%s' "$creator" | jq -r .id)

ugc-cli creator identity \
  --creator "$creator_id" \
  --platform tiktok \
  --external-id creator_handle \
  --profile-url https://tiktok.com/@creator_handle

ugc-cli assignment create \
  --campaign "$campaign_id" \
  --brief "$brief_id" \
  --creator "$creator_id" \
  --payment-owner echo \
  --compensation-minor 25000 \
  --currency USD \
  --shipping-required
```

### Publish through a provider

```bash
ugc-cli campaign publish "$campaign_id" \
  --brief "$brief_id" \
  --connection CONNECTION_ID

ugc-cli sync run
ugc-cli sync outbox
```

Provider calls are durable outbox operations. Failures retry and eventually move to `dead`; replay is explicit:

```bash
ugc-cli sync replay OUTBOX_ID
```

### Shipping

```bash
ugc-cli shipment update \
  --assignment ASSIGNMENT_ID \
  --status ready_to_ship \
  --product-variant blue

ugc-cli shipment update \
  --assignment ASSIGNMENT_ID \
  --status shipped \
  --carrier ups \
  --tracking TRACKING_NUMBER
```

### Submission, asset, and QC

```bash
submission=$(ugc-cli submission add --assignment ASSIGNMENT_ID)
submission_id=$(printf '%s' "$submission" | jq -r .id)

asset=$(ugc-cli asset import ./creator-video.mp4 \
  --submission "$submission_id" \
  --role final)
asset_id=$(printf '%s' "$asset" | jq -r .id)

ugc-cli asset qc "$asset_id" \
  --mime-prefix video/ \
  --min-duration-ms 5000 \
  --max-duration-ms 60000 \
  --aspect-ratios '9:16'

ugc-cli submission review "$submission_id" \
  --status approved
```

Revision requests preserve previous submissions and enqueue provider feedback:

```bash
ugc-cli submission review "$submission_id" \
  --status revision_requested \
  --feedback 'Replace the opening shot and keep the product label visible.'
```

### Usage rights

```bash
ugc-cli rights grant \
  --assignment ASSIGNMENT_ID \
  --asset "$asset_id" \
  --owner Example \
  --license-type commercial \
  --organic \
  --paid-ads \
  --editing \
  --channels tiktok,reels \
  --territories US \
  --starts-at 2026-07-26T00:00:00Z \
  --expires-at 2027-07-26T00:00:00Z \
  --model-release \
  --music-cleared

ugc-cli rights check \
  --assignment ASSIGNMENT_ID \
  --channel tiktok \
  --paid
```

The result is an explicit `allowed` decision with blocking reasons.

### Payments

`payment_owner` on an assignment is `provider`, `echo`, or `none`. The idempotency key combines assignment, submission, amount, and currency, preventing duplicate payment records.

```bash
ugc-cli payment create \
  --assignment ASSIGNMENT_ID \
  --submission SUBMISSION_ID \
  --amount-minor 25000 \
  --currency USD

ugc-cli payment status PAYMENT_ID paid \
  --external-id stripe-or-provider-payment-id
```

The CLI records payment state; actual Stripe execution belongs in a provider gateway and is intentionally not performed from a local operator process.

### Messaging

```bash
ugc-cli message send \
  --assignment ASSIGNMENT_ID \
  --channel provider \
  --body 'Your product has shipped.'

ugc-cli message list --assignment ASSIGNMENT_ID
```

### Polling and webhooks

```bash
ugc-cli sync connection CONNECTION_ID

ugc-cli webhook serve \
  --connection CONNECTION_ID \
  --bind 127.0.0.1:8787
```

The webhook endpoint is:

```text
POST /webhooks/CONNECTION_ID
```

A single payload can also be ingested from a file or stdin:

```bash
ugc-cli webhook ingest \
  --connection CONNECTION_ID \
  --file event.json \
  --signature HEX_SIGNATURE
```

### Operations

```bash
ugc-cli diagnostics
ugc-cli audit --kind campaign --id CAMPAIGN_ID
ugc-cli webhook log --connection CONNECTION_ID
ugc-cli sync outbox --status dead
```

All command output is JSON, making the CLI suitable for shell automation and agent tooling.

## State machines

Invalid transitions fail instead of silently overwriting state.

- campaign: draft → ready → published → sourcing → active → completed
- brief: draft → approved → archived
- assignment: invited/applied → accepted → shipping/production → submitted → approved → licensed → paid → completed
- submission: received → ingesting/QC → pending review → approved/rejected/revision requested
- shipment: awaiting address → ready to ship → shipped → delivered
- payment: pending → processing → paid, with explicit failed/refunded/disputed states

Every mutation writes an audit event.

## Storage

SQLite uses WAL mode and contains canonical records, outbox operations, webhook inbox, settings, and audit events. Assets are content-addressed by SHA-256 under `UGC_ASSET_DIR`; importing the same file reuses the same stored bytes.

## Security boundaries

- provider secrets are resolved from environment variables or regular, non-symlink, owner-only files
- Skarbiec-expanded files are read only at the provider, Weles, or Brama call boundary
- webhook signatures are verified against the raw request body
- duplicate provider events are rejected by a unique event key
- provider asset URLs should be imported immediately; local content-addressed assets are canonical
- the built-in webhook listener is plain HTTP and should run behind a TLS reverse proxy
- payment ownership prevents Echo and a marketplace from both paying the same assignment

## Provider-specific integrations

Billo and Insense do not have public developer contracts implemented here. Add a native adapter only after obtaining official API documentation, credentials, webhook signing rules, and permission to automate the account. Until then, use the manual adapter. Weles may execute a specifically reviewed and supported trajectory on an authorized account, but it is not a generic browser-scraping fallback and does not replace a provider contract.
