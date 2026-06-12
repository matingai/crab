#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

version="${CRAB_VERSION:-v$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')}"
target="${CRAB_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
bundle="${CRAB_DESKTOP_BUNDLE:-}"
extension=""
asset_suffix=""

case "${target}" in
  *apple-darwin*)
    bundle="${bundle:-dmg}"
    extension="dmg"
    ;;
  *windows*)
    bundle="${bundle:-nsis}"
    extension="exe"
    asset_suffix="-setup"
    ;;
  *)
    echo "Unsupported desktop installer target: ${target}" >&2
    echo "Desktop installers currently support macOS DMG and Windows NSIS setup EXE." >&2
    exit 1
    ;;
esac

case "${bundle}" in
  dmg)
    extension="dmg"
    asset_suffix=""
    ;;
  nsis)
    extension="exe"
    asset_suffix="-setup"
    ;;
  *)
    echo "Unsupported Tauri desktop bundle: ${bundle}" >&2
    echo "Use CRAB_DESKTOP_BUNDLE=dmg on macOS or CRAB_DESKTOP_BUNDLE=nsis on Windows." >&2
    exit 1
    ;;
esac

next_env="desktop-shell/next-env.d.ts"
next_env_backup="$(mktemp)"
next_env_had_file=0
if [[ -f "${next_env}" ]]; then
  cp "${next_env}" "${next_env_backup}"
  next_env_had_file=1
fi

restore_next_env() {
  if [[ "${next_env_had_file}" == "1" ]]; then
    cp "${next_env_backup}" "${next_env}"
  else
    rm -f "${next_env}"
  fi
  rm -f "${next_env_backup}"
}

trap restore_next_env EXIT

rm -rf "desktop-shell/src-tauri/target/release/bundle/${bundle}"

(
  cd desktop-shell
  npm run tauri:release -- --bundles "${bundle}"
)

bundle_dir="desktop-shell/src-tauri/target/release/bundle"
installer="$(find "${bundle_dir}" -type f -name "*.${extension}" | sort | head -n 1)"
if [[ -z "${installer}" ]]; then
  echo "No .${extension} installer found in ${bundle_dir}" >&2
  find "${bundle_dir}" -maxdepth 3 -type f >&2 || true
  exit 1
fi

mkdir -p dist
asset_name="crab-desktop-${version}-${target}${asset_suffix}.${extension}"
cp "${installer}" "dist/${asset_name}"

if command -v shasum >/dev/null 2>&1; then
  (cd dist && shasum -a 256 "${asset_name}" > "${asset_name}.sha256")
else
  (cd dist && sha256sum "${asset_name}" > "${asset_name}.sha256")
fi

echo "Created dist/${asset_name}"
echo "Created dist/${asset_name}.sha256"
