#!/usr/bin/env bash

set -u

package_spec=$1

if output=$(cd "${RUNNER_TEMP:-/tmp}" && npm view "$package_spec" version 2>&1); then
  echo published
elif grep -Eq '^npm (ERR!|error) code E404$' <<< "$output"; then
  echo missing
else
  echo '::error::npm view failed' >&2
  while IFS= read -r line; do
    printf 'npm view output: %s\n' "$line" >&2
  done <<< "$output"
  exit 1
fi
