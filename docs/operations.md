# UGC CLI operations

- **Configuration:** the fleet database resolved through Stado, the asset
  directory, actor, and optional provider/service settings.
- **State:** the fleet ledger, content-addressed files, hashed portal tokens,
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

## Damaged persisted JSON

Record exports, outbox reads, webhook logs and audit logs refuse malformed JSON
in `records.data`, `outbox.payload`, `webhook_events.payload` and
`audit_events.details`. The error names the column, original parser cause and
stored text. These values were written as serialized JSON; damaged text is not
returned as a successful JSON string or rewritten by reading it. Treat the
error as private evidence because it includes retained content.

Use `ugc-cli standalone export FILE`, `ugc-cli sync outbox`,
`ugc-cli webhook log --connection ID` and `ugc-cli audit --kind KIND --id ID`
with the required global `--actor` and `--asset-dir` arguments. A refused read
is not an empty ledger, proof of delivered provider work or permission to replay
an external operation. Recover the exact stored record from authoritative data
or a private backup, then repeat the read; this interface does not invent a
replacement value.

Every other ledger read, such as `ugc-cli creator list` or
`ugc-cli creator show ID`, refuses a record whose `records.data` cannot be
read with `KIND ID cannot be read` and the parser cause. `ugc-cli standalone
import-creators FILE` matches each seed's email, ignoring case, against every
stored creator and reports `created` and `existing` ids; when that creator list
cannot be read the import stops with the same error at the seed it was
matching, before creating it, instead of creating every emailed creator again.
Seeds earlier in the file that were already imported stay imported.
`tests/creators/import.rs` (`cargo test --test creator_import`) runs both on a
fleet database of its own.

## Real stored-data qualification

`tests/storage/json.rs` drives the actual compiled CLI and its existing Stado
fleet-database adapter. It creates its own creator through the CLI and owned
outbox, webhook and audit records in the actual database, damages each JSON
column, verifies refusal without changing those bytes, restores the original
bytes and reads recovery. Cleanup independently removes owned rows and the
creator, verifies their absence and records cleanup errors.

Run only against a dedicated qualification database. Set
`UGC_TEST_DATABASE_NAME` to that database's actual name and
`UGC_TEST_SOURCE_REVISION` to the clean candidate commit; database credentials
and certificates use the normal UGC CLI connection contract. The test verifies
`current_database()` before creating records.

```sh
cargo test --test stored_json -- --ignored --nocapture
```

Reports under `target/real-tests/stored-json/` retain the source revision,
candidate binary digest, command arguments, exits, output and cleanup failures.
The deliberately damaged records belong only to that run. This qualification
covers persisted reads through the CLI, not provider delivery, webhook signature
verification, graphical rendering or a public marketplace launch.

## Conversations

`ugc-cli standalone conversation-create` opens with the operator's
`--message`, or with an invitation to the campaign the conversation
names, read once when the conversation is created: a campaign that cannot be
read refuses the creation with the store's cause instead of sending a
collaboration invitation that names no campaign.

The operator server's `POST /api/conversations` takes the same fields as
`conversation-create`: `creator_id`, optional `campaign_id`, `brief_id`,
`offered_compensation_minor`, `initial_message` and `shipping_required`, and a
required `currency`. `POST /api/conversations/{id}/messages` takes `body` and a
required `channel`, as `conversation-send --channel` does. A request without
either is refused as invalid JSON naming the missing field; no currency or
channel is assumed.

An inbound creator message (`ugc-cli standalone conversation-receive` and the
portal's message endpoint) is judged by Brama as one typed choice on
`POST /v1/decisions`, alias `decision-model`, among `opt_out`, `declined`,
`accepted`, `interested`, `pricing`, `submitted`, `question` and `other`, in
any language. The chosen intent moves the conversation, chooses the automatic
reply and is stored on the inbound message. The call is signed as `ugc-cli
analysis analyze` signs: `BRAMA_URL`, `UGC_BRAMA_AGENT_ID` and
`UGC_BRAMA_SIGNING_SECRET_SOURCE` must be set; an unset one refuses the
message with `<NAME> is not set: an inbound creator message's intent is
judged by Brama, …` before anything is stored. A Brama refusal, an answer
without the question or a choice outside the asked intents refuses the
message with what Brama sent; nothing is guessed from words in the message.

Without Brama, the operator states the intent:
`ugc-cli standalone conversation-receive ID --body TEXT --channel CHANNEL
--intent INTENT` records the message with that intent and moves the
conversation exactly as Brama's choice would; Brama is not asked and none of
its settings is read. An intent outside the eight names above is refused with
every name listed, before anything is stored. The portal's message endpoint
has no operator, so it is always judged by Brama.

`ugc-cli analysis analyze --kind KIND --id ID` reviews the record with one
reviewer instruction that names the record's kind; the model weighs what a
review of that kind needs.

`cargo test --test conversations` runs the built CLI on a SQLite fleet
database it creates under `target/real-tests/`, named by
`UGC_CLI_DATABASE_URL`, with every Brama setting removed. One journey opens a
conversation on a campaign and reads back an invitation naming the campaign
and its product, and a conversation on a campaign that does not exist is
refused with `campaign not found` and stores nothing. The other records an
operator-stated intent and reads the conversation's status, stage and messages
back; it checks that the automatic reply says no offer is recorded on a
conversation without one and states the recorded offer on one with
`--offer-minor`, then checks the refusals of an unknown intent, a reply Brama
would judge and a ledger movement without a key. Each run writes
`report.json` with the source revision, the binary digest and every command's
exit and output.

## Connections

`ugc-cli connection add` and `connection edit` name a connection's secrets
with `--token-source` and `--webhook-secret-source` only; `--token-env` and
`--webhook-secret-env` are refused by the argument parser as unexpected
arguments. `cargo test --test option_spellings` runs both refused spellings
on a SQLite fleet database it creates and checks no connection was stored.

## Creator portal access

`ugc-cli standalone portal-create --creator ID` issues an access that never
expires; `--days N` makes it expire N days after it is created, and a value
that is not positive is refused with `portal validity days must be positive`
before anything is stored. No validity is assumed for the operator.
`cargo test --test portal_validity` creates both kinds on a SQLite fleet
database it creates and checks the refusal leaves the creator's accesses
unchanged.

## Onboarding

When the campaign the ledger stamped as its first success can no longer be
read back, `ugc-cli onboarding` stops at `awaiting_campaign` and its note says
which campaign and why, for example `The recorded campaign <id> can no longer
be read out of this ledger: campaign not found: <id>`.
`cargo test --test onboarding_campaign` runs that walk on a SQLite fleet
database it creates under `target/real-tests/`, after removing the stamped
campaign's record, and checks the status and the note.

## Ledger

`ugc-cli standalone ledger-fund`, `ledger-release`, `ledger-settle` and
`ledger-reverse` each require `--idempotency-key KEY`: the caller names the
movement, and repeating a command with the same key records it once. A
missing key is refused by the argument parser; none is generated.

## Operator approval and verification

The authenticated operator screen's **Approve brief** control calls
`POST /api/briefs/{id}/approve`, using the same transition as
`ugc-cli brief approve ID`. An already approved or archived brief is refused.
**Record creator verification** posts the explicitly entered JSON object to
`POST /api/creators/{id}/verify`, using `ugc-cli creator verify ID --metadata JSON`'s
service operation. Invalid JSON or a non-object is refused. This records the
operator's evidence; it does not contact an identity provider or certify the
truth of that evidence.

Both controls share the operator token and the existing result area, which
shows the request, HTTP status and cause. A transport error or refused response
is not approval. CLI reads of the same brief or creator expose the retained
state.

`tests/lifecycle/approvals.rs` creates its own campaign, brief and creator
through the actual CLI, verifies authenticated HTTP mutations through CLI
readback, checks unauthorized approval, repeated approval and non-object
verification refusals, and independently removes its owned rows.
Use the same isolated fleet database as the operator server. In addition to
`UGC_TEST_DATABASE_NAME` and `UGC_TEST_SOURCE_REVISION`, declare
`UGC_TEST_OPERATOR_ORIGIN`, `UGC_TEST_OPERATOR_TOKEN_FILE`,
`UGC_TEST_DEPLOYMENT_EVIDENCE`, `UGC_TEST_CURRENCY` and `UGC_TEST_SERVICE_TYPE`.
The retained deployment evidence must identify the running server; its HTTP
responses do not attest their revision.

```sh
cargo test --test operator_approvals -- --ignored --nocapture
```

Reports under `target/real-tests/operator-approvals/` retain source and binary
identity, the declaration evidence, requests, CLI results and cleanup failures.
The token is read from its file, not recorded in the report. A separate Weles
journey must observe the graphical controls on a dedicated host; HTTP readback
does not prove rendering or external creator verification.

## Operator screen writes

`ugc-cli standalone serve --bind HOST:PORT` requires the request limits it
enforces: `--max-header-line-bytes`, `--max-header-count` and
`--max-body-bytes`; zero is refused by the argument parser and none is assumed. A non-loopback bind
also requires `--operator-token-source` (`env:NAME` or `file:PATH`), whose
value every operator `/api/` request, read or write, must then carry as
`Authorization: Bearer TOKEN`; the creator portal's `/api/portal/` routes and
self-registration's `/api/register` do not use it.

`tests/operator/screen.rs` (`cargo test --test operator_screen`) starts the
actual server on a free loopback port with its own SQLite fleet database and
token, takes the three limits from `UGC_JOURNEY_MAX_HEADER_LINE_BYTES`,
`UGC_JOURNEY_MAX_HEADER_COUNT` and `UGC_JOURNEY_MAX_BODY_BYTES` (refused by
name when unset), and checks through CLI reads that a write without the token
stores nothing, that a campaign is created, renamed and cancelled, an empty
edit is refused, a brief is created, approved and archived, and a creator is
created, edited and removed. Its report under `target/real-tests/operator-screen/`
keeps every CLI command and HTTP request with its status and answer.
