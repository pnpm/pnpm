#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
registry=https://registry.npmjs.org/

read_package_identity() {
  local package_dir=$1
  local manifest="$package_dir/package.json"
  if [ ! -f "$manifest" ]; then
    echo "::error::Missing package manifest: $manifest" >&2
    return 1
  fi
  jq -er '
    [(.publishConfig.name // .name), .version]
    | select(all(.[]; type == "string" and length > 0))
    | @tsv
  ' "$manifest"
}

publication_state() {
  "$script_dir/npm-package-publication-state.sh" "$1"
}

stage_packages() {
  local layer=$1
  local tag=$2
  shift 2

  if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
    {
      printf '### npm staged publication: %s\n\n' "$layer"
      printf '| Package | Stage ID |\n'
      printf '| --- | --- |\n'
    } >> "$GITHUB_STEP_SUMMARY"
  fi

  local package_dir package_path name version package_spec state stage_output stage_id
  local staged_count=0
  for package_dir in "$@"; do
    IFS=$'\t' read -r name version < <(read_package_identity "$package_dir")
    package_spec="$name@$version"
    state=$(publication_state "$package_spec")
    case "$state" in
      published)
        echo "$package_spec is already published; skipping"
        if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
          printf '| `%s` | already published |\n' "$package_spec" >> "$GITHUB_STEP_SUMMARY"
        fi
        ;;
      missing)
        package_path=$(cd "$package_dir" && pwd -P)
        stage_output=$(pnpm stage publish "$package_path/" \
          --registry "$registry" \
          --npmrc-auth-file /dev/null \
          --tag "$tag" \
          --access public \
          --provenance \
          --no-git-checks \
          --reporter=silent \
          --json)
        if ! stage_id=$(jq -er --arg name "$name" \
          '.[$name].stageId | strings | select(length > 0)' <<< "$stage_output"); then
          echo "::error::pnpm stage publish returned no stage ID for $package_spec" >&2
          while IFS= read -r line; do
            printf 'stage output: %s\n' "$line" >&2
          done <<< "$stage_output"
          return 1
        fi
        echo "Staged $package_spec with ID $stage_id"
        if [ -n "${GITHUB_STEP_SUMMARY:-}" ]; then
          printf '| `%s` | `%s` |\n' "$package_spec" "$stage_id" >> "$GITHUB_STEP_SUMMARY"
        fi
        staged_count=$((staged_count + 1))
        ;;
      *)
        echo "::error::Unexpected npm publication state for $package_spec" >&2
        return 1
        ;;
    esac
  done

  if [ -n "${GITHUB_STEP_SUMMARY:-}" ] && [ "$staged_count" -gt 0 ]; then
    {
      printf '\nApprove every staged package above with interactive 2FA using '
      printf '`pnpm stage approve <stage-id>`. '
      printf 'Wait until the workflow finishes, then approve the dependency layers in release order.\n'
    } >> "$GITHUB_STEP_SUMMARY"
  fi
}

case "${1:-}" in
  stage)
    if [ "$#" -lt 4 ]; then
      echo "Usage: $0 stage <layer> <tag> <package-dir>..." >&2
      exit 1
    fi
    shift
    stage_packages "$@"
    ;;
  *)
    echo "Usage: $0 stage <layer> <tag> <package-dir>..." >&2
    exit 1
    ;;
esac
