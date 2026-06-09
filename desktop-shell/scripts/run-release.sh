#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

cd "${ROOT_DIR}"

npm run build

if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

cargo run --release --manifest-path src-tauri/Cargo.toml
