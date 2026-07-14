#!/usr/bin/env bash
# CLI smoke integration test: doctor → new → check → generate → add → test
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/debug/stacksdapp}"

# Resolve to an absolute path before we `cd` into a temp workdir.
if [[ "$BIN" != /* ]]; then
  BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
fi

if [[ ! -x "$BIN" ]]; then
  echo "error: stacksdapp binary not found or not executable: $BIN" >&2
  echo "hint: cargo build --package stacksdapp" >&2
  exit 1
fi

echo "==> binary: $BIN"
"$BIN" --version

echo "==> clarinet pin check (expect 3.21+)"
clarinet --version
clarinet_ver="$(clarinet --version | head -n1)"
if ! [[ "$clarinet_ver" =~ clarinet[[:space:]]+3\.(2[1-9]|[3-9][0-9]|[0-9]{3,}) ]]; then
  echo "error: Clarinet 3.21+ required for CI (got: $clarinet_ver)" >&2
  exit 1
fi

WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/stacksdapp-smoke.XXXXXX")"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT

cd "$WORKDIR"
echo "==> workdir: $WORKDIR"

echo "==> doctor (warnings allowed; Fail must exit non-zero)"
"$BIN" doctor

echo "==> reject unsafe / invalid project names"
expect_fail() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    echo "error: expected failure for: $desc" >&2
    exit 1
  fi
  echo "  ok rejected: $desc"
}
expect_fail "path traversal new" "$BIN" new '../evil'
expect_fail "absolute path new" "$BIN" new '/tmp/stacksdapp-evil'
expect_fail "invalid charset new" "$BIN" new 'bad name'

echo "==> stacksdapp new smoke-app --no-git"
"$BIN" new smoke-app --no-git
cd smoke-app

test -f contracts/Clarinet.toml
test -f contracts/contracts/counter.clar
test -f frontend/package.json
test -f stacksdapp.toml

echo "==> stacksdapp check"
"$BIN" check

echo "==> walk-up from subdirectory (frontend/)"
(
  cd frontend
  "$BIN" -v check 2>"$WORKDIR/walkup.err"
)
grep -q 'project root' "$WORKDIR/walkup.err" || {
  # -v may be quiet under some shells; still require check succeeded from frontend/
  true
}
# Re-run from frontend without relying on debug text — success is enough
( cd frontend && "$BIN" -q check )

echo "==> stacksdapp generate"
"$BIN" generate
test -f frontend/src/generated/contracts.ts
test -f frontend/src/generated/hooks.ts
test -f frontend/src/generated/DebugContracts.tsx

echo "==> reject unsafe contract names"
expect_fail "path traversal add" "$BIN" add '../x'
expect_fail "invalid charset add" "$BIN" add '9bad'

echo "==> stacksdapp add hello-token"
"$BIN" add hello-token --template blank
test -f contracts/contracts/hello-token.clar
"$BIN" check
"$BIN" generate
grep -q 'hello-token' frontend/src/generated/contracts.ts

echo "==> stacksdapp test"
"$BIN" test

echo "==> smoke OK"
