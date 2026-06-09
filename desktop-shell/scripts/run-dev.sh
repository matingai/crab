#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LOG_DIR="${ROOT_DIR}/.logs"
WEB_LOG="${LOG_DIR}/next-dev.log"

mkdir -p "${LOG_DIR}"

cd "${ROOT_DIR}"

npm run dev >"${WEB_LOG}" 2>&1 &
VITE_PID=$!

cleanup() {
  if kill -0 "${VITE_PID}" >/dev/null 2>&1; then
    kill "${VITE_PID}" >/dev/null 2>&1 || true
    wait "${VITE_PID}" >/dev/null 2>&1 || true
  fi
}

trap cleanup EXIT INT TERM

for _ in $(seq 1 60); do
  if curl -fsS http://127.0.0.1:1420/ >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! curl -fsS http://127.0.0.1:1420/ >/dev/null 2>&1; then
  echo "next dev server did not become ready. log: ${WEB_LOG}" >&2
  exit 1
fi

if [[ -f "${HOME}/.cargo/env" ]]; then
  # shellcheck disable=SC1090
  source "${HOME}/.cargo/env"
fi

cargo run --manifest-path src-tauri/Cargo.toml
