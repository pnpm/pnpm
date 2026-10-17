#!/usr/bin/env bash
# Prints the mutable tags the Docker workflow points at a published release,
# one "<target> <source>" pair per line.
#
# Usage: docker-image-tags.sh <image> <version> <alpine> < release-tags
#
# stdin lists the tag names of the published stable releases, one per line.
# `<major>` and `latest` move only for the highest stable version in that
# major and overall, because maintenance releases of an older line are
# published after newer ones. `<alpine>` is "true" when the release has
# Alpine images, which get `<major>-alpine` and `alpine` the same way.

set -euo pipefail

image=$1
version=$2
alpine=$3
major=${version%%.*}

stable=$(sed -n 's/^v\([0-9]*\.[0-9]*\.[0-9]*\)$/\1/p')
if [ -z "$stable" ]; then
  echo "no stable releases on stdin" >&2
  exit 1
fi
newest=$(printf '%s\n' "$stable" "$version" | sort -V | tail -n 1)
newest_in_major=$(printf '%s\n' "$stable" "$version" | grep "^${major}\." | sort -V | tail -n 1)

print_tag() {
  echo "${image}:$1 ${image}:${version}"
  if [ "$alpine" = "true" ]; then
    echo "${image}:$2 ${image}:${version}-alpine"
  fi
}

if [ "$version" = "$newest_in_major" ]; then
  print_tag "$major" "${major}-alpine"
fi
if [ "$version" = "$newest" ]; then
  print_tag latest alpine
fi
