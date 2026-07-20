#!/usr/bin/env bash
# Production hardening regression: dry-run idempotence, config validation, parser edge cases.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/debug/stacksdapp}"
if [[ "$BIN" != /* ]]; then
  BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
fi

if [[ ! -x "$BIN" ]]; then
  echo "error: stacksdapp binary not found: $BIN" >&2
  exit 1
fi

pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1"; exit 1; }

echo "==> production hardening checks using $BIN"

echo "==> unit/property tests (deployer, parser, shell, scaffold)"
cargo test -p stacksdapp-deployer -q
cargo test -p stacksdapp-parser -q
cargo test -p stacksdapp-shell -q
cargo test -p stacksdapp-scaffold fuzz_validation -q
pass "crate-level hardening tests"

echo "==> invalid defaults.network rejected"
WORKDIR="$(mktemp -d "${TMPDIR:-/tmp}/stacksdapp-hardening.XXXXXX")"
cleanup() { rm -rf "$WORKDIR"; }
trap cleanup EXIT
cd "$WORKDIR"
"$BIN" new harden-app --no-git -q
(
  cd harden-app
  cat > stacksdapp.toml <<'EOF'
[defaults]
network = "staging"
EOF
  if "$BIN" deploy --dry-run 2>/dev/null; then
    fail "deploy accepted invalid defaults.network=staging"
  fi
  if "$BIN" dev 2>/dev/null; then
    fail "dev accepted invalid defaults.network=staging"
  fi
)
pass "invalid config network rejected"

echo "==> deploy --dry-run does not mutate Clarinet.toml"
(
  cd harden-app
  clarinet_hash_before=$(shasum -a 256 contracts/Clarinet.toml | awk '{print $1}')
  # devnet dry-run may fail without node; snapshot restore should still leave Clarinet.toml intact
  "$BIN" deploy --dry-run -q 2>/dev/null || true
  clarinet_hash_after=$(shasum -a 256 contracts/Clarinet.toml | awk '{print $1}')
  if [[ "$clarinet_hash_before" != "$clarinet_hash_after" ]]; then
    fail "Clarinet.toml changed after deploy --dry-run"
  fi
)
pass "dry-run leaves Clarinet.toml unchanged"

echo "==> add rejects pathological contract names"
if "$BIN" add '../evil' --template blank >/dev/null 2>&1; then
  fail "accepted path traversal contract name"
fi
if "$BIN" add "$(printf 'a%.0s' {1..41})" --template blank >/dev/null 2>&1; then
  fail "accepted overlong contract name"
fi
pass "contract name edge cases rejected"

echo "==> production hardening checks OK"
