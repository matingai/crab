#!/usr/bin/env bash
set -euo pipefail

repo="matingai/crab"
install_dir="${CRAB_INSTALL_DIR:-${HOME}/.local/bin}"
version="${CRAB_VERSION:-}"

usage() {
  cat <<'USAGE'
Install Crab from GitHub Releases.

Usage:
  scripts/install.sh [--version v0.1.2] [--bin-dir ~/.local/bin]

Environment:
  CRAB_VERSION      Release tag to install. Defaults to the latest GitHub release.
  CRAB_INSTALL_DIR  Directory for the crab binary. Defaults to ~/.local/bin.
USAGE
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --version)
      version="${2:-}"
      shift 2
      ;;
    --bin-dir)
      install_dir="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need curl
need tar

if [[ -z "${version}" ]]; then
  version="$(
    curl -fsSL "https://api.github.com/repos/${repo}/releases?per_page=1" |
      sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' |
      head -n 1
  )"
fi

if [[ -z "${version}" ]]; then
  echo "failed to resolve latest Crab release version" >&2
  exit 1
fi

os="$(uname -s)"
arch="$(uname -m)"
case "${os}:${arch}" in
  Darwin:arm64|Darwin:aarch64)
    target="aarch64-apple-darwin"
    ;;
  Darwin:x86_64|Darwin:amd64)
    target="x86_64-apple-darwin"
    ;;
  Linux:x86_64|Linux:amd64)
    target="x86_64-unknown-linux-gnu"
    ;;
  *)
    echo "unsupported platform: ${os} ${arch}" >&2
    echo "available release targets: macOS arm64, macOS x64, Linux x64, Windows x64" >&2
    exit 1
    ;;
esac

package="crab-${version}-${target}"
archive="${package}.tar.gz"
base_url="https://github.com/${repo}/releases/download/${version}"
tmpdir="$(mktemp -d)"
trap 'rm -rf "${tmpdir}"' EXIT

cd "${tmpdir}"
echo "Downloading ${archive}"
curl -fSLO "${base_url}/${archive}"
curl -fSLO "${base_url}/${archive}.sha256"

if command -v shasum >/dev/null 2>&1; then
  shasum -a 256 -c "${archive}.sha256"
elif command -v sha256sum >/dev/null 2>&1; then
  sha256sum -c "${archive}.sha256"
else
  echo "warning: no sha256 checker found; skipping checksum verification" >&2
fi

tar -xzf "${archive}"
mkdir -p "${install_dir}"
install -m 0755 "${package}/crab" "${install_dir}/crab"

echo "Installed crab to ${install_dir}/crab"
"${install_dir}/crab" --version

case ":${PATH}:" in
  *":${install_dir}:"*) ;;
  *)
    echo "Add ${install_dir} to PATH to run crab from any directory."
    ;;
esac
