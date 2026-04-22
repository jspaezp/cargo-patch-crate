#!/usr/bin/env bash
# Regression test: [patch.crates-io] points at a path under target/patch/
# that doesn't exist yet. Pristine `cargo` commands trip on the missing
# path; our patch-crate binary must handle it by populating target/patch/
# before cargo sees it.
#
# Usage: ./test.sh [path-to-cargo-patch-crate-binary]
#   Default binary: ../../../target/release/cargo-patch-crate

set -euo pipefail

FIX_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$FIX_DIR/../../.." && pwd)"
BIN="${1:-$REPO_ROOT/target/release/cargo-patch-crate}"

if [[ ! -x "$BIN" ]]; then
    echo "binary not found: $BIN" >&2
    echo "build with: cargo build --release" >&2
    exit 2
fi

cd "$FIX_DIR"

echo "=== reset fixture state ==="
rm -rf target/patch patches

echo "=== sanity: cargo build should FAIL (broken patch.crates-io) ==="
if cargo build 2>/dev/null; then
    echo "UNEXPECTED: cargo build succeeded without populating target/patch" >&2
    exit 1
fi
echo "OK: cargo build failed as expected"

echo "=== apply-mode: populate target/patch ==="
"$BIN" patch-crate
test -d target/patch/serde_json-1.0.149 || { echo "FAIL: target/patch/serde_json-1.0.149 missing" >&2; exit 1; }

echo "=== cargo build now works ==="
cargo build --quiet
echo "OK"

echo "=== modify + create-patch ==="
echo "// fixture marker" >> target/patch/serde_json-1.0.149/src/lib.rs
"$BIN" patch-crate serde_json
test -f patches/serde_json+1.0.149.patch || { echo "FAIL: patch file missing" >&2; exit 1; }
grep -q "fixture marker" patches/serde_json+1.0.149.patch || { echo "FAIL: patch does not contain marker" >&2; exit 1; }

echo "=== round trip: wipe target/patch, re-apply ==="
rm -rf target/patch
"$BIN" patch-crate
grep -q "fixture marker" target/patch/serde_json-1.0.149/src/lib.rs || { echo "FAIL: marker not restored" >&2; exit 1; }

echo ""
echo "All regression checks passed."
