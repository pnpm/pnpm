#!/usr/bin/env bash
# Runs `cross build` with the given arguments, retrying when it fails.
#
# The release runners occasionally hit transient failures that a plain rerun
# clears: the Apple linker has segfaulted mid-build, and tool downloads can
# time out. Cargo keeps every crate a failed attempt did finish, so a retry
# only redoes the crate that died. This wrapper saves the maintainer from
# waiting on the whole run, then clicking "re-run failed jobs".

set -euo pipefail

attempts=3
delay_seconds=15

for attempt in $(seq 1 "$attempts"); do
  if cross build "$@"; then
    exit 0
  fi
  if [ "$attempt" -lt "$attempts" ]; then
    echo "::warning::cross build failed (attempt $attempt of $attempts), retrying in ${delay_seconds}s" >&2
    sleep "$delay_seconds"
  fi
done

echo "::error::cross build failed after $attempts attempts" >&2
exit 1
