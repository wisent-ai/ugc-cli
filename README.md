# UGC CLI

<!-- wisent-readme-signals:start -->
[![Release](https://img.shields.io/github/v/release/wisent-ai/ugc-cli?display_name=tag&sort=semver)](https://github.com/wisent-ai/ugc-cli/releases)
[![Downloads](https://img.shields.io/github/downloads/wisent-ai/ugc-cli/total)](https://github.com/wisent-ai/ugc-cli/releases)
[![License](https://img.shields.io/github/license/wisent-ai/ugc-cli)](https://github.com/wisent-ai/ugc-cli)
[![Discord](https://img.shields.io/badge/Discord-Join%20Wisent-5865F2?logo=discord&logoColor=white)](https://discord.gg/qRjpkthq54)
<!-- wisent-readme-signals:end -->

**UGC CLI is a local campaign system of record for creator discovery, briefs,
assignments, shipping, messaging, submissions, assets, rights, payments,
publication, attribution, and audit.**

It provides a complete standalone ledger and manual-provider workflow without
requiring a creator marketplace, hosted database, Wisent account, or payment
provider.

[Quick start](#quick-start) · [Command surface](#primary-interfaces) ·
[Safety boundaries](#safety-rights-and-compliance) ·
[Canonical repository](https://github.com/wisent-ai/ugc-cli)

Version `0.1.0` is public development source. Hosted collaboration, verified
network access, native provider integrations, money movement, and retained
compliance operation are not implied by the local executable.

## Problem and intended users

Creator campaigns connect personal data, briefs, conversations, physical
shipments, media files, review decisions, usage rights, compensation, publication
URLs, and performance metrics. Spreadsheets and inboxes make it difficult to
prove consent, prevent publication without rights, reconcile what was promised,
or move the record between service providers.

UGC CLI serves:

- **brand and agency operators** coordinating campaigns in a portable local
  workspace;
- **creator managers** retaining profiles, outreach, opt-outs, assignments, and
  conversations;
- **reviewers and rights owners** approving briefs, submissions, technical
  quality, and exact usage grants;
- **finance and compliance operators** recording—not necessarily executing—
  payout state, escrow transitions, publications, attribution, and audit events;
- **developers** integrating authorized providers through explicit adapters,
  webhooks, and idempotent outbox processing.

## Product boundaries

### Included

- local campaign, brief, creator, identity, assignment, shipment, submission,
  asset, rights, payment, publication, attribution, and audit records;
- bundled SQLite system of record and content-addressed local asset storage;
- standalone creator discovery, outreach conversations, portal tokens, workflow,
  dashboard, import/export, and local HTTP surfaces;
- deterministic matching and explicit opt-out handling;
- technical asset checks plus separate human review states;
- rights grants and publication checks;
- manual provider connection, outbox, webhook, and replay contracts;
- optional scoped integration points for Brama, Weles, and Skarbiec.

### Explicit non-goals

- UGC CLI does not scrape private creator data, evade platform controls, buy fake
  engagement, conceal sponsorship, or authorize reuse beyond recorded rights.
- Local payment and ledger records do not move money, file tax forms, settle a
  marketplace balance, or establish that a creator was paid.
- Creator verification records operator evidence; it is not identity, audience,
  fraud, or legal verification unless an authorized provider contract says so.
- Technical QC does not replace human creative review or rights approval.
- Publication must fail closed without the required approved brief, submission,
  and valid rights scope.
- The public repository must not contain creator personal data, addresses,
  messages, media, contracts, credentials, campaign plans, payout details, or
  provider-specific private automation.
- Hosted collaboration, managed integrations, payouts, and retained compliance
  evidence are separate services and must fail closed when unavailable.

### Supported environment and current capability

| Surface | Requirement | Current state |
|---|---|---|
| Local CLI and SQLite ledger | Rust compatible with `Cargo.lock` | Implemented |
| Local asset store | writable private directory | Implemented |
| Standalone portal/API | explicit loopback bind and local workspace | Implemented local surface |
| Manual provider adapter/outbox | explicit connection | Implemented contract |
| Brama/Weles/Skarbiec paths | separately configured scoped services | Optional |
| Native marketplace integrations | provider authorization and contract review | Not generally included |
| Real payment rails | payment provider and compliance operation | Not implemented by local ledger |
| Hosted team workspace/directory | managed entitlement | Separate service surface |

## Core use cases

### Create and plan a campaign

- **Actor:** a brand or agency operator.
- **Initial state:** campaign name, brand, product, markets, channels, currency,
  and optional budget/deadline are explicit.
- **Outcome:** the local ledger creates a campaign that can receive versioned
  briefs and assignments.
- **Boundary:** campaign creation contacts no creator and commits no spend.

### Match and engage creators

- **Actor:** an authorized campaign operator.
- **Initial state:** consented or lawfully held creator profiles, identities,
  campaign criteria, and an outreach policy exist.
- **Outcome:** deterministic discovery and conversation records preserve why a
  creator was considered, contacted, accepted, or opted out.
- **Boundary:** native marketplace or messaging access requires a separately
  authorized adapter; opt-outs must remain effective.

### Review content and usage rights

- **Actor:** a reviewer and rights owner.
- **Initial state:** an assignment, submission, imported content-addressed asset,
  and proposed usage scope exist.
- **Outcome:** technical QC, human review, and a rights receipt remain distinct;
  publication checks can reject missing or expired scope.
- **Boundary:** ownership and legal sufficiency remain human/legal decisions; a
  hash proves retained bytes, not that the uploader owned them.

### Record compensation and publication

- **Actor:** an authorized finance or publication operator.
- **Initial state:** assignment, amount/currency, rights, approved content, and
  an external transaction or publication fact exist.
- **Outcome:** the local ledger retains state transitions, publication metadata,
  performance metrics, attribution, and audit evidence.
- **Boundary:** the ledger never substitutes for payment-provider confirmation,
  tax/compliance review, platform disclosure, or creator consent.

## How UGC CLI works

```text
campaign -> brief -> creator match -> assignment -> shipment
                                      │
                                      ▼
conversation -> submission -> content-addressed asset -> QC -> human review
                                                        │
                                                        ▼
                payment record <- rights grant -> publication gate
                         │                              │
                         └──────── audit / export ──────┘
```

SQLite is authoritative for the local operational ledger. The asset directory is
content-addressed storage. Provider connections translate explicit external
events through an outbox/webhook boundary. External marketplaces, carriers,
payment processors, publication platforms, and legal records remain authoritative
for their own facts.

## Quick start

This path creates a disposable local database, records one campaign, and lists
it. It contacts no provider, sends no message, moves no money, and publishes
nothing.

### Prerequisites

- Git;
- the Rust toolchain compatible with `Cargo.lock`;
- a private local directory for any real creator or campaign data.

```bash
git clone https://github.com/wisent-ai/ugc-cli.git
cd ugc-cli
cargo build --locked
export UGC_DB="${TMPDIR:-/tmp}/ugc-cli-quickstart.sqlite"
cargo run --locked -- --db "$UGC_DB" campaign create \
  --name "Disposable quick start" \
  --brand "Example brand" \
  --product "Example product" \
  --markets US \
  --languages en \
  --channels short-video \
  --currency USD
cargo run --locked -- --db "$UGC_DB" campaign list
```

Expected result: `campaign create` prints the new local record and
`campaign list` returns it from the same SQLite file. Remove the disposable file
when finished.

Never use a repository checkout or shared temporary directory for real creator
personal data, messages, addresses, media, contracts, or payout records.

## Primary interfaces

The installed executable is `ugc` and accepts global `--db`, `--asset-dir`, and
`--actor` arguments.

| Command family | Contract |
|---|---|
| `connection` | provider connection lifecycle and health |
| `campaign`, `brief`, `creator`, `assignment` | campaign planning and people/work records |
| `shipment`, `submission`, `asset` | physical and media delivery lifecycle |
| `rights`, `payment`, `message` | rights, compensation records, and communication |
| `sync`, `webhook` | explicit outbox and inbound event processing |
| `standalone` | local discovery, conversations, portal, ledger, publication, metrics, dashboard, serve, import/export |
| `weles`, `skarbiec`, `brama` | bounded optional Wisent integrations |
| `audit`, `diagnostics` | evidence and operator readiness |

Use `ugc <family> --help` for exact subcommands and required fields.

## Safety, rights, and compliance

- Collect the minimum creator data required for a named campaign and legal basis.
- Honor consent, deletion, and opt-out state across imports and adapters.
- Keep creator portal tokens random, hashed at rest, scoped, revocable, and
  separate from operator credentials.
- Require human approval for creative suitability and exact usage scope.
- Record territory, channel, duration, exclusivity, edit rights, paid-media use,
  and expiry explicitly; do not infer them from a generic approval.
- Treat shipment addresses, tax data, payout instruments, private messages, and
  unpublished media as sensitive.
- Disclose sponsorship and platform-required labels; performance metrics do not
  justify fake engagement or platform-policy evasion.
- Reconcile every external payment, shipment, and publication against the
  authoritative provider; an internal status alone is insufficient.

## Operational model

- **Configuration:** explicit database, asset directory, actor, and optional
  provider/service settings.
- **State:** local SQLite ledger, content-addressed files, hashed portal tokens,
  outbox, webhook log, and audit records.
- **Credentials:** references belong in Skarbiec or provider-specific secret
  stores; never export secret values with campaign data.
- **Observability:** diagnostics, connection health, sync outbox, webhook log,
  workflow view, dashboard, and audit trail.
- **Recovery:** use standalone export/import plus private database and asset
  backups; reconcile provider-side transactions before replaying outbox work.
- **Cost:** local operation is not metered; managed workspace seats/campaigns and
  pass-through messaging, shipping, media, provider, and payment costs are
  separate and must be explicit.

## Project status and support

- **Maturity:** public development source, version `0.1.0`.
- **Local contract:** complete standalone campaign ledger, portal, rights gate,
  asset store, audit, export, and manual-provider workflow.
- **Managed contract:** hosted collaboration, verified directory, native
  integrations, payouts, retained compliance evidence, and support are separate;
  no availability is promised by this repository.
- **Issues:** [`wisent-ai/ugc-cli`](https://github.com/wisent-ai/ugc-cli/issues).
- **Security and privacy:** use private GitHub Security Advisories; never attach
  creator records, addresses, messages, media, contracts, credentials, payout
  data, customer plans, or private provider traces to a public issue.
- **License:** Apache License 2.0; see [`LICENSE`](LICENSE).
