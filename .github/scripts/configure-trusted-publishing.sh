#!/usr/bin/env bash
# Registers `.github/workflows/release.yml` as the crates.io trusted publisher
# for every publishable workspace crate that has none yet.
#
# Run it from the repository root after a crate's first publish, with an
# API token that has the `trusted-publishing` endpoint scope:
#
#   CRATES_IO_TOKEN=... .github/scripts/configure-trusted-publishing.sh
#
# A crate that is not on crates.io yet is reported and skipped.
set -euo pipefail

: "${CRATES_IO_TOKEN:?set CRATES_IO_TOKEN to a crates.io API token with the trusted-publishing scope}"

api="https://crates.io/api/v1/trusted_publishing/github_configs"
user_agent="rust-api-stack trusted publishing setup (https://github.com/JedimEmO/rust-api-stack)"
owner="JedimEmO"
repo="rust-api-stack"
workflow="release.yml"
environment="release"

request() {
  curl --silent --show-error --user-agent "$user_agent" \
    --header "Authorization: ${CRATES_IO_TOKEN}" "$@"
}

cargo metadata --no-deps --format-version 1 --locked |
  jq -r '.packages[] | select(.publish != []) | .name' | sort |
  while read -r name; do
    existing=$(request "${api}?crate=${name}")
    if ! jq --exit-status '.github_configs' <<<"$existing" >/dev/null; then
      echo "skip ${name}: $(jq -c '.errors // .' <<<"$existing")"
      continue
    fi
    if jq --exit-status --arg o "$owner" --arg r "$repo" --arg w "$workflow" \
      'any(.github_configs[]; .repository_owner == $o and .repository_name == $r and .workflow_filename == $w)' \
      <<<"$existing" >/dev/null; then
      echo "ok   ${name}: already configured"
      continue
    fi

    body=$(jq -n --arg c "$name" --arg o "$owner" --arg r "$repo" --arg w "$workflow" --arg e "$environment" \
      '{github_config: {crate: $c, repository_owner: $o, repository_name: $r, workflow_filename: $w, environment: $e}}')
    created=$(request --header "Content-Type: application/json" --data "$body" "$api")
    if jq --exit-status '.github_config.id' <<<"$created" >/dev/null; then
      echo "add  ${name}: configured"
    else
      echo "fail ${name}: $(jq -c '.errors // .' <<<"$created")"
    fi
  done
