#!/usr/bin/env bash
# Lists the publishable workspace crates whose current version is not on crates.io yet.
#
# Prints two lines in GitHub Actions output format:
#   packages=-p ras-foo -p ras-bar   (arguments for `cargo publish`)
#   tags=ras-foo-v0.2.0 ras-bar-v0.1.1   (release tags to create after publishing)
# Both are empty when every version is already published.
set -euo pipefail

user_agent="rust-api-stack release workflow (https://github.com/JedimEmO/rust-api-stack)"
index_file=$(mktemp)
trap 'rm -f "$index_file"' EXIT

packages=()
tags=()

while read -r name version; do
  # Sparse index path: names of four or more characters live at ab/cd/<name>.
  lower=${name,,}
  path="${lower:0:2}/${lower:2:2}/${lower}"
  status=$(curl --silent --show-error --output "$index_file" --write-out '%{http_code}' \
    --user-agent "$user_agent" "https://index.crates.io/${path}")

  case "$status" in
    404) published=false ;;
    200)
      if jq --exit-status --slurp --arg v "$version" 'any(.[]; .vers == $v)' "$index_file" >/dev/null; then
        published=true
      else
        published=false
      fi
      ;;
    *)
      echo "crates.io index answered ${status} for ${name}" >&2
      exit 1
      ;;
  esac

  if [ "$published" = false ]; then
    echo "${name} ${version} is not published yet" >&2
    packages+=("-p" "$name")
    tags+=("${name}-v${version}")
  fi
done < <(cargo metadata --no-deps --format-version 1 --locked |
  jq -r '.packages[] | select(.publish != []) | "\(.name) \(.version)"' | sort)

echo "packages=${packages[*]}"
echo "tags=${tags[*]}"
