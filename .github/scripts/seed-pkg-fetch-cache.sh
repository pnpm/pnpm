#!/usr/bin/env bash
# Puts the Node.js binaries that pkg embeds into pkg-fetch's cache, taken from
# a pkg-fetch CI run instead of a pkg-fetch release.
#
# No pkg-fetch release carries Node.js 24.22.1, so pkg cannot download it. The
# binaries exist as artifacts of the pkg-fetch run below. The pkg-fetch patch in
# __patches__ maps node24 to 24.22.1 and pins each binary's sha256. pkg-fetch
# accepts a cached binary only if its hash is pinned, and this script checks the
# hashes first, so a mismatch fails here instead of sending pkg-fetch into a
# build of Node.js from source.
#
# TODO: delete this script, its workflow steps, and the pkg-fetch patch once a
# pkg-fetch release maps node24 to 24.22.1 or later.

set -euo pipefail

pkg_fetch_repo=yao-pkg/pkg-fetch
pkg_fetch_run_id=39791143445
node_version=v24.22.1

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)

pkg_fetch_dir=$(cd "$repo_root/pnpm/artifacts/exe" && node -p "
  const pkgDir = require('path').dirname(require.resolve('pkg/package.json'))
  require('path').dirname(require.resolve('@yao-pkg/pkg-fetch/package.json', { paths: [pkgDir] }))
")
pkg_fetch_version=$(jq -er .version "$pkg_fetch_dir/package.json")
cache_dir="${PKG_CACHE_PATH:-$HOME/.pkg-cache}/v$(cut -d. -f1-2 <<<"$pkg_fetch_version")"
expected_shas="$pkg_fetch_dir/lib-es5/expected-shas.json"

download_dir=$(mktemp -d)
trap 'rm -rf "$download_dir"' EXIT
mkdir -p "$cache_dir"

for config in "$repo_root"/pnpm/package-*.json; do
  # A target looks like node24-linuxstatic-arm64.
  target=$(jq -er '.pkg.targets | if length == 1 then .[0] else error("expected one target") end' "$config")
  node_major=${target%%-*}
  platform_arch=${target#*-}
  binary="node-$node_version-$platform_arch"
  # pkg-fetch's CI names Windows artifacts "windows", its binaries "win".
  artifact="$node_major-${platform_arch/#win-/windows-}"

  expected=$(jq -er --arg name "$binary" '.[$name] // empty' "$expected_shas") || {
    echo "::error::The pkg-fetch patch pins no hash for $binary" >&2
    exit 1
  }

  for attempt in 1 2 3; do
    gh run download "$pkg_fetch_run_id" --repo "$pkg_fetch_repo" --name "$artifact" --dir "$download_dir/$artifact" && break
    if [ "$attempt" = 3 ]; then
      echo "::error::Could not download $artifact from $pkg_fetch_repo run $pkg_fetch_run_id" >&2
      exit 1
    fi
    rm -rf "${download_dir:?}/$artifact"
    sleep 10
  done
  actual=$(sha256sum "$download_dir/$artifact/$binary" | cut -d' ' -f1)
  if [ "$actual" != "$expected" ]; then
    echo "::error::$binary from $pkg_fetch_repo run $pkg_fetch_run_id has sha256 $actual, expected $expected" >&2
    exit 1
  fi

  install -m 755 "$download_dir/$artifact/$binary" "$cache_dir/fetched-$node_version-$platform_arch"
  echo "Seeded $cache_dir/fetched-$node_version-$platform_arch"
done
