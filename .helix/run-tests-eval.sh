#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$REPO_ROOT"

# Prevalidation resets the repo to arbitrary SHAs. Mounts can also hide the image's
# pre-built node_modules. Stale node_modules from another commit still satisfies
# -d node_modules/.pnpm but breaks verify-deps-before-run (.npmrc). Always sync install.
pnpm install --frozen-lockfile

# Packages excluded from the test run for two reasons:
#
# 1. REGISTRY_MOCK: require a live Verdaccio registry mock (PNPM_REGISTRY_MOCK_PORT)
#    which is unreliable in the CI container.
#
# 2. LFS_FIXTURES: their test fixtures are binary .tgz files tracked via Git LFS.
#    Without LFS the files are 129-byte pointer stubs and tests fail with bad
#    tarball/checksum errors.
# Package names follow pnpm v11 workspace layout (scoped @pnpm/<domain>.<pkg>).
EXCLUDED_PACKAGES=(
  # -- REGISTRY_MOCK --
  "@pnpm/cache.commands"
  "@pnpm/building.commands"
  "@pnpm/exec.commands"
  "@pnpm/patching.commands"
  "@pnpm/installing.client"
  "@pnpm/installing.commands"
  "@pnpm/installing.package-requester"
  "pnpm"
  "@pnpm/releasing.commands"
  "@pnpm/deps.inspection.commands"
  "@pnpm/deps.inspection.outdated"
  "@pnpm/deps.inspection.list"
  "@pnpm/store.commands"
  "@pnpm/engine.pm.commands"
  # -- LFS_FIXTURES --
  "@pnpm/fetching.tarball-fetcher"
  "@pnpm/store.cafs"
  "@pnpm/resolving.local-resolver"
  # -- TIMEOUT/INFRA --
  "@pnpm/store.connection-manager"
  # -- STALE_SNAPSHOT (existing-solution branch has outdated snapshot) --
  "@pnpm/cli.default-reporter"
)

# Build pnpm --filter exclusion args
EXCLUDE_ARGS=()
for pkg in "${EXCLUDED_PACKAGES[@]}"; do
  EXCLUDE_ARGS+=("--filter=!${pkg}")
done

if [[ $# -eq 0 ]]; then
  # No arguments — run all tests except excluded packages
  pnpm run --no-sort --workspace-concurrency=2 -r "${EXCLUDE_ARGS[@]}" _test
else
  # With arguments — comma-separated list of test file paths
  IFS=',' read -ra FILES <<< "$1"

  for file in "${FILES[@]}"; do
    # Trim surrounding whitespace
    file="${file#"${file%%[![:space:]]*}"}"
    file="${file%"${file##*[![:space:]]}"}"

    # Resolve to absolute path
    if [[ "$file" = /* ]]; then
      abs_file="$file"
    else
      abs_file="$REPO_ROOT/$file"
    fi

    # Walk up the directory tree to find the nearest package root
    dir="$(dirname "$abs_file")"
    pkg_dir=""
    while [[ "$dir" != "$REPO_ROOT" && "$dir" != "/" ]]; do
      if [[ -f "$dir/package.json" ]]; then
        pkg_dir="$dir"
        break
      fi
      dir="$(dirname "$dir")"
    done

    if [[ -n "$pkg_dir" ]]; then
      # Run jest in the package directory, filtering to the specific file
      # Use pnpm exec so args reach jest (pnpm run _test -- injects "--" and breaks testPathPattern)
      (cd "$pkg_dir" && pnpm exec jest --testPathPattern="$abs_file" --passWithNoTests)
    else
      echo "Warning: could not find package root for: $file" >&2
    fi
  done
fi
