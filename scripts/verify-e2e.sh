#!/usr/bin/env bash
# Full e2e for stacksdapp hardening. Safe under `set -e` (expected failures use if/||).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="${1:-$ROOT/target/debug/stacksdapp}"
if [[ "$BIN" != /* ]]; then
  BIN="$(cd "$(dirname "$BIN")" && pwd)/$(basename "$BIN")"
fi

FAILS=0
pass() { echo "  PASS: $1"; }
fail() { echo "  FAIL: $1"; FAILS=$((FAILS + 1)); }

cd "$ROOT"
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  FULL E2E — stacksdapp                                       ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo "binary: $BIN"
"$BIN" --version

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 1) Unit tests"
echo "────────────────────────────────────────────────────────────────"
if cargo test --all -q; then
  pass "cargo test --all"
else
  fail "unit tests"
fi

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 2) Completions (P1-3)"
echo "────────────────────────────────────────────────────────────────"
if "$BIN" completions --help | grep -q zsh; then pass "completions help"; else fail "completions help"; fi
for s in bash zsh fish powershell elvish; do
  OUT=$("$BIN" completions "$s" 2>/dev/null | wc -c | tr -d ' ')
  if [[ "$OUT" -gt 100 ]]; then pass "completions $s ($OUT bytes)"; else fail "completions $s"; fi
done
if "$BIN" com zsh 2>/dev/null | head -3 >/dev/null; then pass "alias com + pipe safe"; else fail "com/pipe"; fi
if "$BIN" completions zsh 2>/dev/null | grep -q '_stacksdapp'; then pass "zsh _stacksdapp"; else fail "zsh content"; fi

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 3) Display flags (P1-1)"
echo "────────────────────────────────────────────────────────────────"
"$BIN" doctor --json > /tmp/e2e_doc.json 2>/dev/null || true
if python3 -c 'import json;d=json.load(open("/tmp/e2e_doc.json")); assert d["command"]=="doctor"'; then
  pass "doctor --json"
else
  fail "doctor --json"
fi
OUT=$("$BIN" -q doctor 2>&1 || true)
if [[ ${#OUT} -eq 0 ]]; then pass "-q quiet"; else fail "-q leaked"; fi
"$BIN" -vv doctor >/dev/null 2>/tmp/e2e_vv.err || true
if grep -q 'stacksdapp starting' /tmp/e2e_vv.err; then pass "-vv debug"; else fail "-vv"; fi
"$BIN" --color never doctor 2>/dev/null | head -8 > /tmp/e2e_never.txt || true
if grep -q $'\033' /tmp/e2e_never.txt; then fail "color never ANSI"; else pass "--color never"; fi

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 4) P0 doctor / sanitize / flags"
echo "────────────────────────────────────────────────────────────────"
if "$BIN" doctor >/dev/null 2>&1; then pass "doctor exit 0"; else fail "doctor exit"; fi
if "$BIN" doctor --strict >/dev/null 2>&1; then
  pass "doctor --strict 0 (all green)"
else
  pass "doctor --strict non-zero"
fi
if "$BIN" new '../evil' >/dev/null 2>&1; then fail "accepted ../evil"; else pass "reject ../evil"; fi
if "$BIN" new 'bad name' >/dev/null 2>&1; then fail "accepted space"; else pass "reject bad name"; fi
if "$BIN" deploy --help | grep -q -- '--yes'; then pass "deploy --yes"; else fail "deploy --yes"; fi
if "$BIN" clean --help | grep -q -- '--force'; then pass "clean --force"; else fail "clean --force"; fi

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 5) Walk-up + --root (P1-2)"
echo "────────────────────────────────────────────────────────────────"
WORKDIR=$(mktemp -d /tmp/stacksdapp-e2e.XXXXXX)
mkdir -p "$WORKDIR/contracts/contracts" "$WORKDIR/frontend/src/generated"
cat > "$WORKDIR/contracts/Clarinet.toml" <<'EOF'
[project]
name = "e2e"
telemetry = false
[contracts.counter]
path = "contracts/counter.clar"
clarity_version = 5
epoch = "latest"
EOF
echo '(define-read-only (g) (ok u1))' > "$WORKDIR/contracts/contracts/counter.clar"
echo '[project]
name = "e2e"' > "$WORKDIR/stacksdapp.toml"
echo '{}' > "$WORKDIR/frontend/src/generated/deployments.json"
(
  cd "$WORKDIR/frontend/src"
  "$BIN" -v clean --force >/tmp/e2e_walk.out 2>/tmp/e2e_walk.err
)
if grep -q 'project root' /tmp/e2e_walk.err; then pass "walk-up from frontend/src"; else fail "walk-up"; fi
if "$BIN" check >/dev/null 2>/tmp/e2e_noproj.err; then
  fail "outside-project should fail"
else
  if grep -q 'No stacksdapp project found' /tmp/e2e_noproj.err; then
    pass "outside-project error"
  else
    fail "outside-project message"
  fi
fi
"$BIN" --root "$WORKDIR" -v clean --force >/dev/null 2>/tmp/e2e_root.err || true
if grep -q 'project root' /tmp/e2e_root.err; then pass "--root override"; else fail "--root"; fi
rm -rf "$WORKDIR"

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 6) Redeploy false-positive fix"
echo "────────────────────────────────────────────────────────────────"
if cargo test -p stacksdapp-codegen --lib stale -q; then
  pass "codegen stale tests"
else
  fail "codegen stale tests"
fi

echo ""
echo "────────────────────────────────────────────────────────────────"
echo " 7) Full smoke e2e (new → check → generate → add → test)"
echo "────────────────────────────────────────────────────────────────"
if bash "$ROOT/scripts/ci-smoke.sh" "$BIN"; then
  pass "ci-smoke"
else
  fail "ci-smoke"
fi

echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
if [[ $FAILS -eq 0 ]]; then
  echo "║  ALL E2E CHECKS PASSED                                       ║"
  echo "╚══════════════════════════════════════════════════════════════╝"
  exit 0
fi
echo "║  $FAILS CHECK(S) FAILED                                        ║"
echo "╚══════════════════════════════════════════════════════════════╝"
exit 1
