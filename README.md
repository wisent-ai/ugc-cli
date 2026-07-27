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

On Unix, the database, its SQLite sidecars, exported backups, and stored assets are forced to owner-only permissions. Symbolic-link paths are rejected for the database, asset directory, and backup output.

## Standalone mode: complete local workflow

Standalone mode requires no creator marketplace, Weles, Brama, Skarbiec, Stripe, email provider, hosted database, or external API. SQLite is the system of record, assets stay in the local content-addressed library, conversations use the local creator portal, payouts use the internal escrow ledger plus an operator-recorded offline settlement, and publication metrics can be entered locally.

What works locally:

- creator-directory JSON import, filtering, scoring, and campaign matching
- deterministic outreach and two-way conversation threads
- intent classification, opt-out handling, and safe automatic replies
- one-time creator portal capability links
- offer acceptance, optional shipping-address collection, and assignment creation
- browser media upload into the local content-addressed library
- asset hashing, technical QC, human review, revisions, and rights gates
- immutable idempotent escrow transfers and offline-settlement references
- publication registration, cumulative metric snapshots, attribution events, and campaign performance reports
- deterministic workflow advancement with explicit blockers
- local operator HTTP API, creator HTML portal, JSON export/import, audit, and dashboard

### Local creator directory

Import the bundled example or a JSON array using the same schema:

```bash
ugc-cli standalone import-creators examples/creators.json

ugc-cli standalone discover \
  --markets US \
  --languages en \
  --niches beauty \
  --channels instagram,tiktok
```

Each creator can carry local scoring evidence in `metadata`: `followers`, `engagement_rate`, `completed_campaigns`, `response_rate`, `portfolio_count`, `base_rate_minor`, and `channels`. Numeric values may be JSON numbers or numeric strings. Hard filters run before scoring, and the result explains matched and missing signals.

### Create and launch a campaign

Create a campaign and approved brief with the regular `campaign` and `brief` commands. Then launch local discovery and outreach:

```bash
ugc-cli standalone launch \
  --campaign CAMPAIGN_ID \
  --brief APPROVED_BRIEF_ID \
  --niches beauty \
  --channels instagram \
  --offer-minor 25000 \
  --shipping-required \
  --limit 10
```

Launch performs these operations without an external integration:

1. scores the local creator directory,
2. skips creators already contacted for the campaign,
3. creates a canonical conversation and deterministic outreach message,
4. creates an expiring creator portal token,
5. returns every conversation and portal token as JSON.

Inspect or continue a thread:

```bash
ugc-cli standalone conversation-list --campaign CAMPAIGN_ID
ugc-cli standalone conversation-messages CONVERSATION_ID

ugc-cli standalone conversation-receive CONVERSATION_ID \
  --body 'I am interested. What is the rate?'

ugc-cli standalone conversation-send CONVERSATION_ID \
  --body 'The approved offer is 25000 USD minor units.'
```

Inbound messages are classified as interested, accepted, pricing, question, submitted, declined, opt-out, or other. `STOP`, unsubscribe, and equivalent Polish phrases close the conversation without another automated reply.

Accepting a conversation creates the standalone assignment. Add `--shipping-required` for a physical-product campaign:

```bash
ugc-cli standalone conversation-accept CONVERSATION_ID --shipping-required
```

### Local creator portal and operator API

Start the single-machine service:

```bash
ugc-cli standalone serve
```

To let new creators enroll themselves locally:

```bash
ugc-cli standalone serve --allow-registration
```

The enrollment form is then available at `http://127.0.0.1:8765/register`. Registration requires a unique email, prevalidates globally unique platform identities, creates the canonical creator profile and self-reported identity records, and returns a creator portal token once. Self-registered creators and identities remain unverified and excluded from discovery until an operator verifies the profile; verification promotes the associated self-reported identities:

```bash
ugc-cli creator verify CREATOR_ID \
  --metadata '{"verification_source":"manual portfolio review"}'
```

Open:

```text
http://127.0.0.1:8765/
http://127.0.0.1:8765/portal/CREATOR_PORTAL_TOKEN
```

The creator portal can:

- reply in a campaign conversation,
- accept the recorded offer,
- enter a shipping address when a physical product is required,
- see assignment state and compensation,
- upload a media file from the browser.

Operator JSON endpoints:

| Method | Path | Purpose |
|---|---|---|
| GET | `/health` | local service health |
| GET | `/register` | optional creator self-registration form |
| POST | `/api/register` | optional creator self-registration |
| GET | `/api/dashboard` | counts and attention queue |
| GET | `/api/creators` | creator directory |
| GET | `/api/campaigns` | campaign list |
| GET | `/api/conversations` | unified local inbox |
| POST | `/api/conversations` | start outreach |
| GET/POST | `/api/conversations/{id}/messages` | read or send messages |
| POST | `/api/conversations/{id}/accept` | create an assignment |
| POST | `/api/submissions/{id}/review` | human submission decision |
| GET | `/api/portal/{token}` | creator portal data |
| POST | `/api/portal/{token}/reply` | creator reply |
| POST | `/api/portal/{token}/accept` | creator acceptance |
| POST | `/api/portal/{token}/shipping` | save an assignment shipping address |
| POST | `/api/portal/{token}/submission` | stream a browser media upload |

The default listener is loopback-only. Binding to a non-loopback address fails unless an operator bearer token is supplied:

```bash
ugc-cli standalone serve \
  --bind 0.0.0.0:8765 \
  --operator-token-source 'file:.ugc/operator-token'
```

Creator portal tokens are random capabilities stored only as SHA-256 hashes. The plaintext token is returned once. Operator token files must satisfy the same owner-only, non-symlink checks as provider secrets.

### Physical-product shipping

For assignments accepted with `--shipping-required`, the creator portal collects the recipient address and marks the shipment ready. Inspect it and record dispatch:

```bash
ugc-cli shipment list --assignment ASSIGNMENT_ID

ugc-cli shipment update \
  --assignment ASSIGNMENT_ID \
  --status shipped \
  --carrier CARRIER \
  --tracking TRACKING_NUMBER
```

After delivery is confirmed:

```bash
ugc-cli shipment update \
  --assignment ASSIGNMENT_ID \
  --status delivered
```

The workflow will not move a physical-product assignment into production until delivery is recorded. Shipping addresses remain in the owner-only local database.

### Submission, QC, review, and rights

After the creator submits through the portal, run technical QC:

```bash
ugc-cli asset qc ASSET_ID \
  --mime-prefix video/ \
  --aspect-ratios '9:16'

ugc-cli submission review SUBMISSION_ID \
  --status approved
```

QC moves a received submission to `pending_review`. Approval remains a human decision. Record usage rights before publication or escrow release:

```bash
ugc-cli rights grant \
  --assignment ASSIGNMENT_ID \
  --asset ASSET_ID \
  --owner Example \
  --license-type commercial \
  --organic \
  --paid-ads \
  --editing \
  --channels instagram,tiktok \
  --territories US \
  --starts-at 2026-07-26T00:00:00Z \
  --model-release \
  --music-cleared
```

### Standalone escrow and offline payout

The ledger is an immutable transfer journal. Balances are derived from posted transfers instead of stored mutable totals.

```text
external:funding
  → escrow:ASSIGNMENT_ID
  → creator:CREATOR_ID
  → external:offline_payout
```

Advance the workflow after approval and rights. It creates the payment record when the assignment becomes licensed:

```bash
ugc-cli standalone workflow --campaign CAMPAIGN_ID --apply
```

Fund the assignment escrow:

```bash
ugc-cli standalone ledger-fund \
  --assignment ASSIGNMENT_ID \
  --amount-minor 25000 \
  --currency USD \
  --idempotency-key funding-reference
```

Release an approved payment into the creator ledger:

```bash
ugc-cli standalone ledger-release \
  --payment PAYMENT_ID \
  --idempotency-key release-reference
```

Or let the workflow release every fully validated, funded pending payment:

```bash
ugc-cli standalone workflow \
  --campaign CAMPAIGN_ID \
  --apply \
  --release-payments
```

After paying the creator by bank transfer, cash, or another offline rail, record the real settlement reference:

```bash
ugc-cli standalone ledger-settle \
  --payment PAYMENT_ID \
  --reference BANK_OR_RECEIPT_REFERENCE \
  --idempotency-key settlement-reference
```

Check balances and journal entries:

```bash
ugc-cli standalone ledger-balance \
  --account escrow:ASSIGNMENT_ID \
  --currency USD

ugc-cli standalone ledger-list --assignment ASSIGNMENT_ID
```

Standalone mode processes approval, escrow, release, idempotency, reversals, settlement state, and reconciliation locally. It cannot physically move money through a bank without a payment rail; instead it requires the operator's real offline settlement reference before marking the payment paid.

### Publication and performance tracking

Publication is blocked unless the submission is approved, the asset belongs to it, and rights allow the requested channel and paid/organic mode:

```bash
ugc-cli standalone publication-add \
  --assignment ASSIGNMENT_ID \
  --submission SUBMISSION_ID \
  --asset ASSET_ID \
  --platform instagram \
  --channel instagram \
  --post-id PLATFORM_POST_ID \
  --url https://instagram.com/p/POST
```

Capture cumulative metrics:

```bash
ugc-cli standalone metrics-capture \
  --publication PUBLICATION_ID \
  --views 10000 \
  --likes 800 \
  --comments 40 \
  --shares 60 \
  --saves 100 \
  --clicks 240 \
  --conversions 18 \
  --revenue-minor 90000 \
  --spend-minor 25000 \
  --currency USD
```

Counters cannot decrease between snapshots. Record independently deduplicated conversion or revenue evidence:

```bash
ugc-cli standalone attribution-add \
  --publication PUBLICATION_ID \
  --event-type order \
  --external-id ORDER_ID \
  --value-minor 5000 \
  --currency USD
```

Campaign report:

```bash
ugc-cli standalone performance --campaign CAMPAIGN_ID
```

The report includes creator cost, media spend, revenue, views, engagement, clicks, conversions, engagement rate, click rate, conversion rate, and ROAS.

### Autonomous workflow and attention queue

Dry inspection never mutates state:

```bash
ugc-cli standalone workflow --campaign CAMPAIGN_ID
```

Safe apply mode advances only transitions already supported by evidence:

```bash
ugc-cli standalone workflow --campaign CAMPAIGN_ID --apply
```

It can advance campaign state, move shipped physical-product assignments into production, advance approved submissions into licensed assignments, create payment records, release explicitly funded escrow when requested, complete paid assignments, and complete campaigns that have a recorded publication. Human review, rights creation, escrow funding, and real offline settlement remain explicit gates.

```bash
ugc-cli standalone dashboard
```

The dashboard reports unanswered conversations, pricing/questions requiring operator follow-up, pending reviews, payments awaiting release or settlement, and publications missing metrics. A non-automated operator reply clears the follow-up flag.

### Backup and restore

Export all canonical records:

```bash
ugc-cli standalone export ugc-backup.json
```

Restore them into another standalone database:

```bash
ugc-cli --db restored/ugc.db standalone import ugc-backup.json
```

Assets are separate content-addressed files under `UGC_ASSET_DIR`; copy that directory alongside the JSON export. The import is idempotent by record ID.

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
