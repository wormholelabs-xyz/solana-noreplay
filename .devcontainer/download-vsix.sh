#!/bin/bash
set -euo pipefail

# Usage: download-vsix.sh <publisher> <name> <version> <sha256>
# Downloads a VS Code extension VSIX to /opt/ext/<publisher>.<name>-<version>.vsix.
# Download order: Marketplace API → gallery.vsassets.io CDN.
#
# <sha256> formats:
#   HASH                        — universal VSIX; no platform suffix added to URL
#   amd64=HASH,arm64=HASH       — platform-specific VSIX; ?targetPlatform=linux-{arch} appended

publisher="$1"
name="$2"
version="$3"
sha256_arg="$4"

if [ -z "${version}" ] || [ -z "${sha256_arg}" ]; then
  echo "ERROR: version or sha256 not set for ${publisher}.${name}" >&2
  echo "Run .devcontainer/fetch-vscode-extension-shas.sh to populate build args." >&2
  exit 1
fi

mkdir -p /opt/ext
out="/opt/ext/${publisher}.${name}-${version}.vsix"

is_valid_zip() { unzip -t "$1" > /dev/null 2>&1; }

try_url() {
  local url="$1" ua="${2:-}"
  if [ -n "${ua}" ]; then
    curl -fsSL --retry 2 -A "${ua}" "${url}" -o "${out}" 2>/dev/null
  else
    curl -fsSL --retry 2 "${url}" -o "${out}" 2>/dev/null
  fi
  [ -s "${out}" ] && is_valid_zip "${out}"
}

# Resolve sha256 and optional platform URL suffix.
sha256=""
platform_suffix=""

if echo "${sha256_arg}" | grep -qF '='; then
  # Platform-specific map: "amd64=HASH,arm64=HASH"
  arch=$(dpkg --print-architecture)
  sha256=$(echo "${sha256_arg}" | tr ',' '\n' | grep "^${arch}=" | cut -d= -f2-)
  if [ -z "${sha256}" ]; then
    echo "ERROR: No sha256 for ${publisher}.${name} on arch ${arch}" >&2
    exit 1
  fi
  case "${arch}" in
    amd64) platform_suffix="?targetPlatform=linux-x64"   ;;
    arm64) platform_suffix="?targetPlatform=linux-arm64" ;;
  esac
else
  sha256="${sha256_arg}"
fi

marketplace_api="https://marketplace.visualstudio.com/_apis/public/gallery/publishers/${publisher}/vsextensions/${name}/${version}/vspackage${platform_suffix}"
marketplace_cdn="https://${publisher}.gallery.vsassets.io/_apis/public/gallery/publisher/${publisher}/extension/${name}/${version}/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage${platform_suffix}"

if try_url "${marketplace_api}" "Mozilla/5.0 (compatible)"; then
  :
elif try_url "${marketplace_cdn}"; then
  :
else
  echo "ERROR: Failed to download ${publisher}.${name} from Marketplace" >&2
  exit 1
fi

echo "${sha256}  ${out}" | sha256sum -c
