#!/usr/bin/env bash
# Quality gate local: o mesmo que o CI roda.
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== cargo fmt --check"
cargo fmt --check
echo "== cargo clippy"
cargo clippy --all-targets -- -D warnings
echo "== cargo test"
cargo test
echo "== gate OK"
