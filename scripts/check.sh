#!/usr/bin/env bash
# The checks CI runs, in the order that fails fastest.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

echo "== rustfmt =="
cargo fmt --all -- --check

echo "== clippy =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test =="
cargo test

echo "== typecheck =="
pnpm -r typecheck

echo "== eslint =="
pnpm -r lint

echo "== vitest =="
pnpm -r test

echo "== extension bundles =="
pnpm build:extension
pnpm --filter @localtrack/chrome-extension build:firefox

echo "All checks passed."
