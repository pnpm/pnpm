#!/usr/bin/env bash
# Checks that each pnpr binary given as an argument accepts every documented
# storage.database.url scheme.
#
# `pnpr config check` connects to the configured database. Nothing listens at
# the network URLs below, so a check passes when pnpr reaches the connect
# step: that proves the binary includes the scheme's driver. A missing driver
# or a rejected scheme fails before connecting, with a different message.

set -euo pipefail

check_dir=$(mktemp -d)
trap 'rm -rf "$check_dir"' EXIT

urls=(
  'sqlite:./metadata.sqlite'
  'libsql://database.example.test'
  'http://127.0.0.1:8080'
  'https://database.example.test'
  'postgres://example:secret@database.example.test/registry'
  'postgresql://example:secret@database.example.test/registry'
  'mysql://example:secret@database.example.test/registry'
)

status=0
for bin in "$@"; do
  for url in "${urls[@]}"; do
    printf 'storage:\n  path: ./storage\n  database:\n    url: %s\n' "$url" > "$check_dir/db-check.yaml"
    if output=$("$bin" --config "$check_dir/db-check.yaml" config check 2>&1 >/dev/null); then
      continue
    fi
    # miette wraps the message across lines that start with a box-drawing bar.
    message=$(printf '%s' "$output" | sed 's/│//g' | tr -s '[:space:]' ' ')
    case "$message" in
      *'error trying to connect'* | *'error communicating with database'*) ;;
      *)
        echo "::error::$bin rejected $url"
        printf '%s\n' "$output"
        status=1
        ;;
    esac
  done
done
exit "$status"
