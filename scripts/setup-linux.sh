#!/usr/bin/env bash
set -euo pipefail
project_root="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd)"
"$project_root/scripts/setup-background.sh"
cargo build --manifest-path "$project_root/Cargo.toml" --locked --release
