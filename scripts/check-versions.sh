#!/usr/bin/env bash
# Print workspace crate versions and optionally validate release tag consistency.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
CHECK_TAG=""
REQUIRE_CONSISTENT=0

usage() {
  cat <<'EOF'
Usage: check-versions.sh [--check-tag TAG] [--consistent]

  --check-tag TAG   Fail unless TAG matches cli (stacksdapp) version (v prefix optional)
  --consistent      Fail unless every workspace crate shares the same version
  -h, --help        Show this help
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --check-tag)
      CHECK_TAG="${2:-}"
      shift 2
      ;;
    --consistent)
      REQUIRE_CONSISTENT=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "error: unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

echo "Crate versions (from Cargo.toml):"
printf '%-36s %s\n' "CRATE" "VERSION"
printf '%-36s %s\n' "-----" "-------"

CLI_VER=""
FIRST_VER=""
ALL_SAME=1
FAIL=0

for toml in "$ROOT/cli/Cargo.toml" "$ROOT/crates"/*/Cargo.toml; do
  [[ -f "$toml" ]] || continue
  name=$(awk -F'"' '/^name[[:space:]]*=/ { print $2; exit }' "$toml")
  ver=$(awk -F'"' '/^version[[:space:]]*=/ { print $2; exit }' "$toml")
  printf '%-36s %s\n' "$name" "$ver"
  if [[ -z "$FIRST_VER" ]]; then
    FIRST_VER="$ver"
  elif [[ "$ver" != "$FIRST_VER" ]]; then
    ALL_SAME=0
  fi
  if [[ "$toml" == "$ROOT/cli/Cargo.toml" ]]; then
    CLI_VER="$ver"
  fi
done

echo ""
echo "Release tag should match cli (stacksdapp) version, e.g. v${CLI_VER}"

if [[ "$REQUIRE_CONSISTENT" -eq 1 ]]; then
  if [[ "$ALL_SAME" -ne 1 ]]; then
    echo "error: workspace crate versions are not consistent" >&2
    FAIL=1
  else
    echo "ok: all workspace crates share version ${CLI_VER}"
  fi
fi

if [[ -n "$CHECK_TAG" ]]; then
  normalized="${CHECK_TAG#v}"
  if [[ "$normalized" != "$CLI_VER" ]]; then
    echo "error: tag ${CHECK_TAG} does not match cli version ${CLI_VER}" >&2
    FAIL=1
  else
    echo "ok: tag ${CHECK_TAG} matches cli version ${CLI_VER}"
  fi
fi

exit "$FAIL"
