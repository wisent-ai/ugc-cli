#!/bin/sh
set -eu

: "${WISENT_VERSION:?WISENT_VERSION is required}"
: "${WISENT_SOURCE_DIR:?WISENT_SOURCE_DIR is required}"
: "${WISENT_OUTPUT_DIR:?WISENT_OUTPUT_DIR is required}"
: "${WISENT_PLATFORM:?WISENT_PLATFORM is required}"
: "${WISENT_INPUTS_DIR:?WISENT_INPUTS_DIR is required}"

case "$WISENT_PLATFORM" in
  darwin-arm64|linux-amd64) ;;
  *)
    echo "unsupported release platform: $WISENT_PLATFORM" >&2
    exit 64
    ;;
esac

: "${WISENT_INPUT_ECHO_WEB_DIR:?WISENT_INPUT_ECHO_WEB_DIR is required}"
source_crate="$WISENT_INPUT_ECHO_WEB_DIR/crates/onboarding-client"
[ -f "$source_crate/Cargo.toml" ] || { echo "Echo onboarding client input is incomplete" >&2; exit 66; }
rm -rf "$WISENT_OUTPUT_DIR/dependencies/onboarding-client"
mkdir -p "$WISENT_OUTPUT_DIR/dependencies"
cp -R "$source_crate" "$WISENT_OUTPUT_DIR/dependencies/onboarding-client"
mkdir -p "$WISENT_OUTPUT_DIR/cargo-quality"
export CARGO_INCREMENTAL=0
export SOURCE_DATE_EPOCH=1
export CARGO_TARGET_DIR="$WISENT_OUTPUT_DIR/cargo-quality"
cd "$WISENT_SOURCE_DIR"
cargo check --locked --manifest-path "$WISENT_SOURCE_DIR/Cargo.toml"
