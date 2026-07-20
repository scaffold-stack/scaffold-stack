#!/usr/bin/env bash
# Scaffold a project and verify the generated frontend builds and typechecks.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/debug/stacksdapp}"
if [[ "$BIN" != /* ]]; then
  BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
fi

if [[ ! -x "$BIN" ]]; then
  echo "error: stacksdapp binary not found or not executable: $BIN" >&2
  echo "hint: cargo build --package stacksdapp" >&2
  exit 1
fi

echo "==> frontend CI using $BIN"

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/stacksdapp-frontend.XXXXXX")"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

cd "$WORKDIR"
echo "==> workdir: $WORKDIR"

"$BIN" new frontend-smoke --no-git -q
cd frontend-smoke

echo "==> generate bindings"
"$BIN" -q generate

echo "==> npm run build (Next.js production build + typecheck)"
(
  cd frontend
  npm run build
)

echo "==> npm run typecheck (tsc --noEmit)"
(
  cd frontend
  npm run typecheck
)

echo "==> frontend CI OK"
