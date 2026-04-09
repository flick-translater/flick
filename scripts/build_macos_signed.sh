#!/usr/bin/env bash

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
CERT_NAME="${CERT_NAME:-Flick Self-Signed Code Signing}"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script only supports macOS." >&2
  exit 1
fi

if ! security find-identity -v -p codesigning | grep -F "$CERT_NAME" >/dev/null; then
  echo "Missing codesigning identity: $CERT_NAME" >&2
  echo "Run ./scripts/create_macos_self_signed_cert.sh first." >&2
  exit 1
fi

cd "$REPO_ROOT/frontend"
export APPLE_SIGNING_IDENTITY="$CERT_NAME"

npm run tauri:build
