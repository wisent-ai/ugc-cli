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

The CLI never stores provider secrets in SQLite. Connections store environment-variable names; credentials remain in the process environment.

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

## Provider modes

### Manual

A complete assisted-workflow adapter. Publishing creates a stable external reference and marks operations requiring work in the provider dashboard as `manual_action_required`.

```bash
ugc-cli connection add --name billo-manual --provider manual
```

### HTTP

A native adapter for a provider gateway implementing the contract below.

```bash
export MY_UGC_TOKEN='secret'
export MY_UGC_WEBHOOK_SECRET='secret'

ugc-cli connection add \
  --name primary-provider \
  --provider http \
  --base-url https://ugc-gateway.example.com \
  --token-env MY_UGC_TOKEN \
  --webhook-secret-env MY_UGC_WEBHOOK_SECRET
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

- provider secrets are read from named environment variables
- webhook signatures are verified against the raw request body
- duplicate provider events are rejected by a unique event key
- provider asset URLs should be imported immediately; local content-addressed assets are canonical
- the built-in webhook listener is plain HTTP and should run behind a TLS reverse proxy
- payment ownership prevents Echo and a marketplace from both paying the same assignment

## Provider-specific integrations

Billo and Insense do not have public developer contracts implemented here. Add a native adapter only after obtaining official API documentation, credentials, webhook signing rules, and permission to automate the account. Until then, use the manual adapter rather than browser scraping a logged-in dashboard.
