#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

version="${CRAB_VERSION:-v$(cargo metadata --no-deps --format-version 1 | python3 -c 'import json,sys; print(json.load(sys.stdin)["packages"][0]["version"])')}"
target="${CRAB_TARGET:-$(rustc -vV | awk '/^host:/ { print $2 }')}"
bin_suffix=""
archive_ext="tar.gz"

case "${target}" in
  *windows*)
    bin_suffix=".exe"
    archive_ext="zip"
    ;;
esac

package_name="crab-${version}-${target}"
package_dir="dist/${package_name}"
binary="target/${target}/release/crab${bin_suffix}"

cargo build --release --locked --target "${target}"

rm -rf "${package_dir}"
mkdir -p "${package_dir}/docs" "${package_dir}/scripts"
cp "${binary}" "${package_dir}/"
cp README.md README.zh-CN.md LICENSE "${package_dir}/"
cp docs/INSTALL.md docs/INSTALL.zh-CN.md docs/QUICKSTART.md docs/QUICKSTART.zh-CN.md "${package_dir}/docs/"
cp scripts/install.sh scripts/install.ps1 "${package_dir}/scripts/"

if [[ "${archive_ext}" == "zip" ]]; then
  if command -v zip >/dev/null 2>&1; then
    (cd dist && zip -qr "${package_name}.zip" "${package_name}")
  else
    powershell -NoProfile -Command "Compress-Archive -Path '${package_dir}/*' -DestinationPath 'dist/${package_name}.zip' -Force"
  fi
  archive_path="dist/${package_name}.zip"
else
  tar -czf "dist/${package_name}.tar.gz" -C dist "${package_name}"
  archive_path="dist/${package_name}.tar.gz"
fi

archive_dir="$(dirname "${archive_path}")"
archive_file="$(basename "${archive_path}")"
if command -v shasum >/dev/null 2>&1; then
  (cd "${archive_dir}" && shasum -a 256 "${archive_file}" > "${archive_file}.sha256")
else
  (cd "${archive_dir}" && sha256sum "${archive_file}" > "${archive_file}.sha256")
fi

echo "Created ${archive_path}"
echo "Created ${archive_path}.sha256"
