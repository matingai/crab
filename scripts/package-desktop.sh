#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

version="${CRAB_VERSION:-v$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')}"
target_was_explicit=0
if [[ -n "${CRAB_TARGET:-}" ]]; then
  target="${CRAB_TARGET}"
  target_was_explicit=1
else
  target="$(rustc -vV | awk '/^host:/ { print $2 }')"
fi
bundle="${CRAB_DESKTOP_BUNDLE:-}"
extension=""
asset_suffix=""
platform=""

case "${target}" in
  *apple-darwin*)
    bundle="${bundle:-dmg}"
    extension="dmg"
    platform="macOS"
    ;;
  *windows*)
    bundle="${bundle:-nsis}"
    extension="exe"
    asset_suffix="-setup"
    platform="Windows"
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
    platform="${platform:-macOS}"
    ;;
  nsis)
    extension="exe"
    asset_suffix="-setup"
    platform="${platform:-Windows}"
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

tauri_args=("--bundles" "${bundle}")
build_release_dir="desktop-shell/src-tauri/target/release"
if [[ "${target_was_explicit}" == "1" ]]; then
  tauri_args=("--target" "${target}" "${tauri_args[@]}")
  build_release_dir="desktop-shell/src-tauri/target/${target}/release"
fi

rm -rf "${build_release_dir}/bundle/${bundle}"

(
  cd desktop-shell
  npm run tauri:release -- "${tauri_args[@]}"
)

bundle_dir="${build_release_dir}/bundle"
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
checksum="$(awk '{ print $1 }' "dist/${asset_name}.sha256")"
cat > "dist/${asset_name}.json" <<EOF
{
  "name": "${asset_name}",
  "version": "${version}",
  "target": "${target}",
  "platform": "${platform}",
  "bundle": "${bundle}",
  "kind": "desktop-installer",
  "file": "${asset_name}",
  "sha256": "${checksum}",
  "unsigned_preview": true,
  "install_hint": "Download this installer from the matching GitHub release and verify the .sha256 file before opening it."
}
EOF

echo "Created dist/${asset_name}"
echo "Created dist/${asset_name}.sha256"
echo "Created dist/${asset_name}.json"
