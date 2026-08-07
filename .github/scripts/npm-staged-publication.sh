#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

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

  local package_dir name version package_spec state stage_output stage_id
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
        stage_output=$(pnpm stage publish "${package_dir%/}/" \
          --tag "$tag" \
          --access public \
          --provenance \
          --no-git-checks \
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
      printf 'The workflow will continue after this entire dependency layer is public.\n'
    } >> "$GITHUB_STEP_SUMMARY"
  fi
}

wait_for_packages() {
  local layer=$1
  shift

  local poll_seconds=${NPM_PUBLICATION_POLL_SECONDS:-30}
  local timeout_seconds=${NPM_PUBLICATION_TIMEOUT_SECONDS:-19800}
  if [[ ! $poll_seconds =~ ^[0-9]+$ ]]; then
    echo "::error::NPM_PUBLICATION_POLL_SECONDS must be a non-negative integer" >&2
    return 1
  fi
  if [[ ! $timeout_seconds =~ ^[0-9]+$ ]]; then
    echo "::error::NPM_PUBLICATION_TIMEOUT_SECONDS must be a non-negative integer" >&2
    return 1
  fi

  local package_dir name version package_spec state
  local -a package_specs=()
  for package_dir in "$@"; do
    IFS=$'\t' read -r name version < <(read_package_identity "$package_dir")
    package_specs+=("$name@$version")
  done

  local deadline=$((SECONDS + timeout_seconds))
  local -a pending=()
  while true; do
    pending=()
    for package_spec in "${package_specs[@]}"; do
      state=$(publication_state "$package_spec")
      case "$state" in
        published) ;;
        missing) pending+=("$package_spec") ;;
        *)
          echo "::error::Unexpected npm publication state for $package_spec" >&2
          return 1
          ;;
      esac
    done

    if [ "${#pending[@]}" -eq 0 ]; then
      echo "Every package in $layer is published."
      return
    fi
    if [ "$SECONDS" -ge "$deadline" ]; then
      echo "::error::Timed out waiting for npm approval of the $layer packages:" >&2
      printf '  %s\n' "${pending[@]}" >&2
      return 1
    fi

    echo "Waiting for npm approval of the $layer packages:"
    printf '  %s\n' "${pending[@]}"
    sleep "$poll_seconds"
  done
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
  wait)
    if [ "$#" -lt 3 ]; then
      echo "Usage: $0 wait <layer> <package-dir>..." >&2
      exit 1
    fi
    shift
    wait_for_packages "$@"
    ;;
  *)
    echo "Usage: $0 <stage|wait> ..." >&2
    exit 1
    ;;
esac
