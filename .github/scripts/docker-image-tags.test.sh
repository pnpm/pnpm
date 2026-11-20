#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
image=ghcr.io/pnpm/pnpm
releases=$'v12.14.0\nv12.13.0\nv12.9.1\nv11.28.6\nv11.28.5\nv10.34.7\nv9.15.9\npnpr@0.1.0-alpha.15\nv12.0.0-rc.1'

assert_tags() {
  local version=$1
  local alpine=$2
  local expected=$3
  local actual
  actual=$("$script_dir/docker-image-tags.sh" "$image" "$version" "$alpine" <<< "$releases")
  if [ "$actual" != "$expected" ]; then
    printf 'for %s (alpine=%s)\nexpected:\n%s\ngot:\n%s\n' "$version" "$alpine" "$expected" "$actual" >&2
    exit 1
  fi
}

# The newest release moves every mutable tag. Versions compare numerically,
# so 9.15.9 and 12.9.1 do not outrank it.
assert_tags 12.14.0 true "$image:12 $image:12.14.0
$image:12-alpine $image:12.14.0-alpine
$image:latest $image:12.14.0
$image:alpine $image:12.14.0-alpine"
# A release missing from the list yet, newer than all of it.
assert_tags 12.15.0 false "$image:12 $image:12.15.0
$image:latest $image:12.15.0"
# A maintenance release of an older major moves only its major tag.
assert_tags 11.28.6 false "$image:11 $image:11.28.6"
# An older release within the newest major moves nothing.
assert_tags 12.13.0 true ''

# An empty release list fails instead of treating the version as the newest.
if "$script_dir/docker-image-tags.sh" "$image" 11.28.6 false < /dev/null > /dev/null 2>&1; then
  echo 'expected an empty release list to fail' >&2
  exit 1
fi
