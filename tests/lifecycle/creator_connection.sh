#!/bin/sh
# Real test of the creator and connection lifecycle verbs of `ugc-cli` on the
# fleet database, through the built binary: connection add, edit and remove,
# creator add, edit, identity, identity-remove and remove.
#
# Every record it makes is named with a run-unique id, so no operator record
# is touched, and every one is removed at the end. It checks the refusals: a
# command without --actor, an edit that names no change, and removing a
# connection while a creator identity names it (the refusal names the
# identity). Every command, its exit status and output go to the run's
# report.txt.
#
# Usage: tests/lifecycle/creator_connection.sh   (UGC selects the binary,
#   default target/debug/ugc-cli)
set -eu
cd "$(dirname "$0")/../.."
BIN=${UGC:-target/debug/ugc-cli}
RUN="$(date -u +%Y%m%dT%H%M%SZ)-$$"
ROOT="$PWD/target/real-tests/lifecycle/$RUN"
REPORT="$ROOT/report.txt"
mkdir -p "$ROOT/assets"
echo "revision: $(git rev-parse HEAD)$(git diff --quiet || echo ' (dirty)')" >"$REPORT"
echo "binary: $BIN" >>"$REPORT"
ACTOR="ugc-cli-test-$RUN"

run() {
  expected=$1
  shift
  set +e
  out=$("$BIN" "$@" 2>"$ROOT/stderr" </dev/null)
  status=$?
  set -e
  err=$(cat "$ROOT/stderr")
  printf '$ ugc-cli %s\nexit: %s\nstdout: %s\nstderr: %s\n\n' "$*" "$status" "$out" "$err" >>"$REPORT"
  if [ "$status" -ne "$expected" ]; then
    echo "FAIL: ugc-cli $* exited $status, expected $expected: $err" | tee -a "$REPORT" >&2
    exit 1
  fi
}
ugc() {
  expected=$1
  shift
  run "$expected" --actor "$ACTOR" --asset-dir "$ROOT/assets" --json "$@"
}
check() {
  if [ "$2" != "$3" ]; then
    echo "FAIL: $1: got '$2', expected '$3'" | tee -a "$REPORT" >&2
    exit 1
  fi
  echo "ok: $1 = $2" >>"$REPORT"
}
refused() {
  case "$err" in
    *"$1"*) echo "ok: refused with: $1" >>"$REPORT" ;;
    *) echo "FAIL: expected a refusal containing '$1', got: $err" | tee -a "$REPORT" >&2; exit 1 ;;
  esac
}

run 1 --asset-dir "$ROOT/assets" connection list
refused "--actor is required: name who acts, as the audit record names it"

ugc 0 connection add --name "test connection $RUN" --provider manual
connection=$(echo "$out" | jq -r .id)
ugc 1 connection edit "$connection"
refused "connection edit needs at least one of --name, --base-url, --token-source, --webhook-secret-source, --external-account-id"
ugc 0 connection edit "$connection" --name "edited connection $RUN" --external-account-id "account-$RUN"
ugc 0 connection show "$connection"
check "the connection's name is edited" "$(echo "$out" | jq -r .name)" "edited connection $RUN"
check "its external account is edited" "$(echo "$out" | jq -r .external_account_id)" "account-$RUN"
check "its provider stays" "$(echo "$out" | jq -r .provider)" "manual"

ugc 0 creator add --name "test creator $RUN" --languages pl
creator=$(echo "$out" | jq -r .id)
ugc 1 creator edit "$creator"
refused "creator edit needs at least one of --name, --email, --languages, --markets, --niches"
ugc 0 creator edit "$creator" --name "edited creator $RUN" --markets PL,DE
ugc 0 creator show "$creator"
check "the creator's name is edited" "$(echo "$out" | jq -r '.display_name // .creator.display_name')" "edited creator $RUN"

ugc 0 creator identity --creator "$creator" --connection "$connection" --platform tiktok --external-id "tt-$RUN"
identity=$(echo "$out" | jq -r .id)
ugc 1 connection remove "$connection"
refused "connection $connection cannot be removed while these name it: creator identity $identity"

ugc 0 creator identity-remove "$identity"
ugc 1 creator identity-remove "$identity"
ugc 0 connection remove "$connection"
ugc 1 connection show "$connection"
ugc 0 creator remove "$creator"
ugc 1 creator show "$creator"

touch "$ROOT/passed"
echo "PASS" >>"$REPORT"
echo "PASS: $REPORT"
