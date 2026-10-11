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
