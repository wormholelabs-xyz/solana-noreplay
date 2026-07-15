#!/usr/bin/env bash
set -euo pipefail

# Fetches version-pinned SHA256s for all VS Code extensions and writes them
# directly into .devcontainer/devcontainer.json. All extensions are fetched in
# parallel; results are collected into per-extension temp files and applied
# serially after all downloads complete (no locking required).
#
# Usage: .devcontainer/fetch-vscode-extension-shas.sh [publisher.name ...]
#
# Versions newer than MIN_AGE_HOURS are skipped to reduce supply-chain risk.
# Default is 336h (2 weeks). Override: MIN_AGE_HOURS=48 ./fetch-vscode-extension-shas.sh
#
# Requires: curl, jq, sha256sum (Linux) or shasum (macOS)

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
DEVCONTAINER_JSON="${SCRIPT_DIR}/devcontainer.json"
MIN_AGE_HOURS="${MIN_AGE_HOURS:-336}"

TMPDIR_WORK=$(mktemp -d)
_tmp=""
trap 'rm -rf "${TMPDIR_WORK}"; [ -n "${_tmp}" ] && rm -f "${_tmp}"' EXIT

sha256() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum "$1" | awk '{print $1}'
  else
    shasum -a 256 "$1" | awk '{print $1}'
  fi
}

# Returns the newest version that is at least MIN_AGE_HOURS old. Outputs
# nothing if no version qualifies.
# flags=17: IncludeVersions(1) + IncludeVersionProperties(16), all versions newest-first.
get_marketplace_info() {
  local ext_id="$1"
  local min_age_seconds=$(( MIN_AGE_HOURS * 3600 ))
  curl -fsSL \
    -X POST \
    -H "Content-Type: application/json" \
    -H "Accept: application/json;api-version=3.0-preview.1" \
    "https://marketplace.visualstudio.com/_apis/public/gallery/extensionquery" \
    --data-raw "{\"filters\":[{\"criteria\":[{\"filterType\":7,\"value\":\"${ext_id}\"}],\"pageSize\":1}],\"flags\":17}" \
    | jq -r --argjson min_age "${min_age_seconds}" '
        .results[0].extensions[0].versions
        | map(select(
            ((.properties // []) | map(select(.key == "Microsoft.VisualStudio.Code.PreRelease" and .value == "true")) | length) == 0
          ))
        | map(select((now - (.lastUpdated | gsub("\\.[0-9]+Z$"; "Z") | fromdateiso8601)) >= $min_age))
        | if length == 0 then empty else first | .version, .lastUpdated end
      '
}

is_valid_zip() { unzip -t "$1" > /dev/null 2>&1; }

try_url() {
  local url="$1" outfile="$2" ua="${3:-}"
  if [ -n "${ua}" ]; then
    curl -fsSL --retry 2 -A "${ua}" "${url}" -o "${outfile}" 2>/dev/null
  else
    curl -fsSL --retry 2 "${url}" -o "${outfile}" 2>/dev/null
  fi
  [ -s "${outfile}" ] && is_valid_zip "${outfile}"
}

# Try downloading a platform-specific variant (linux-x64 or linux-arm64) from marketplace.
# Outputs the sha256 on success, empty string on failure.
try_platform_sha() {
  local publisher="$1" name="$2" version="$3" platform="$4"
  local outfile="${TMPDIR_WORK}/${publisher}.${name}.${platform}.vsix"
  local api="https://marketplace.visualstudio.com/_apis/public/gallery/publishers/${publisher}/vsextensions/${name}/${version}/vspackage?targetPlatform=${platform}"
  local cdn="https://${publisher}.gallery.vsassets.io/_apis/public/gallery/publisher/${publisher}/extension/${name}/${version}/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage?targetPlatform=${platform}"
  if try_url "${api}" "${outfile}" "Mozilla/5.0 (compatible)" || try_url "${cdn}" "${outfile}"; then
    sha256 "${outfile}"
  fi
}

download_and_sha() {
  local publisher="$1"
  local name="$2"
  local version="$3"

  local outfile="${TMPDIR_WORK}/${publisher}.${name}.vsix"
  local marketplace_api="https://marketplace.visualstudio.com/_apis/public/gallery/publishers/${publisher}/vsextensions/${name}/${version}/vspackage"
  local marketplace_cdn="https://${publisher}.gallery.vsassets.io/_apis/public/gallery/publisher/${publisher}/extension/${name}/${version}/assetbyname/Microsoft.VisualStudio.Services.VSIXPackage"

  # Probe for platform-specific variants. If linux-x64 and linux-arm64 yield different
  # hashes the extension bundles native binaries and needs per-arch sha256 tracking.
  local hash_x64 hash_arm64
  hash_x64=$(try_platform_sha "${publisher}" "${name}" "${version}" "linux-x64")
  hash_arm64=$(try_platform_sha "${publisher}" "${name}" "${version}" "linux-arm64")

  if [ -n "${hash_x64}" ] && [ -n "${hash_arm64}" ] && [ "${hash_x64}" != "${hash_arm64}" ]; then
    echo "amd64=${hash_x64},arm64=${hash_arm64}"
    return
  fi

  # Universal (pure-JS) extension: download once without a platform suffix.
  if try_url "${marketplace_api}" "${outfile}" "Mozilla/5.0 (compatible)"; then
    :
  elif try_url "${marketplace_cdn}" "${outfile}"; then
    :
  else
    echo "  [${publisher}.${name}] ERROR: marketplace download failed" >&2
    return 1
  fi

  sha256 "${outfile}"
}

# Runs in a background job. On success writes "VERSION\nSHA256" to
# ${TMPDIR_WORK}/<arg_prefix>.result; on skip writes nothing (no result file).
process_ext() {
  local publisher="$1"
  local name="$2"
  local arg_prefix="$3"
  local result_file="${TMPDIR_WORK}/${arg_prefix}.result"

  should_process "${publisher}.${name}" || return 0

  echo "  [${publisher}.${name}] querying..." >&2

  local info version
  info=$(get_marketplace_info "${publisher}.${name}")

  if [ -z "${info}" ]; then
    echo "  [${publisher}.${name}] SKIPPED — no version older than ${MIN_AGE_HOURS}h ($(( MIN_AGE_HOURS / 24 ))d)" >&2
    return 0
  fi

  version=$(echo "${info}" | head -1)
  echo "  [${publisher}.${name}] found v${version}, downloading..." >&2

  local sha
  sha=$(download_and_sha "${publisher}" "${name}" "${version}")

  echo "  [${publisher}.${name}] OK v${version} ${sha}" >&2

  printf '%s\n%s\n' "${version}" "${sha}" > "${result_file}"
}

# Optional positional args filter which extensions to process: publisher.name [publisher.name ...]
# If none given, all extensions are processed.
FILTER=("$@")

should_process() {
  local id="$1"
  if [ "${#FILTER[@]}" -eq 0 ]; then return 0; fi
  for f in "${FILTER[@]}"; do
    [ "$f" = "$id" ] && return 0
  done
  return 1
}

if [ "${#FILTER[@]}" -gt 0 ]; then
  echo "Fetching selected extensions in parallel (min age: ${MIN_AGE_HOURS}h / $(( MIN_AGE_HOURS / 24 ))d): ${FILTER[*]}" >&2
else
  echo "Fetching all extensions in parallel (min age: ${MIN_AGE_HOURS}h / $(( MIN_AGE_HOURS / 24 ))d)..." >&2
fi

PIDS=()

process_ext anthropic claude-code     CLAUDE_CODE_EXT & PIDS+=($!)
process_ext dbaeumer  vscode-eslint   ESLINT_EXT      & PIDS+=($!)
process_ext esbenp    prettier-vscode PRETTIER_EXT    & PIDS+=($!)
process_ext eamodio   gitlens         GITLENS_EXT     & PIDS+=($!)

FAILED=0
for pid in "${PIDS[@]}"; do
  wait "${pid}" || FAILED=$(( FAILED + 1 ))
done

echo "" >&2
if [ "${FAILED}" -gt 0 ]; then
  echo "ERROR: ${FAILED} extension(s) failed — devcontainer.json not updated." >&2
  exit 1
fi

# Apply all results serially — no locking needed since downloads are done.
for result_file in "${TMPDIR_WORK}"/*.result; do
  [ -f "${result_file}" ] || continue
  arg_prefix="$(basename "${result_file}" .result)"
  version="$(sed -n '1p' "${result_file}")"
  sha="$(sed -n '2p' "${result_file}")"
  _tmp=$(mktemp "${DEVCONTAINER_JSON}.XXXXXX")
  jq --arg ver_key "${arg_prefix}_VERSION" \
     --arg sha_key "${arg_prefix}_SHA256" \
     --arg ver "${version}" \
     --arg sha "${sha}" \
     '.build.args[$ver_key] = $ver | .build.args[$sha_key] = $sha' \
     "${DEVCONTAINER_JSON}" > "${_tmp}"
  mv "${_tmp}" "${DEVCONTAINER_JSON}"
done

echo "Done — devcontainer.json updated." >&2
