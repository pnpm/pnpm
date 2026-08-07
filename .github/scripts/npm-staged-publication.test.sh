#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
test_tmp=$(mktemp -d)
trap 'rm -rf "$test_tmp"' EXIT
fake_bin="$test_tmp/bin"
state_dir="$test_tmp/state"
mkdir "$fake_bin" "$state_dir"

create_package() {
  local dir=$1
  local name=$2
  mkdir "$dir"
  cat > "$dir/package.json" <<EOF
{
  "name": "private-workspace-name",
  "version": "1.0.0",
  "publishConfig": {
    "name": "$name"
  }
}
EOF
}

create_package "$test_tmp/staged" '@pnpm/staged'
create_package "$test_tmp/already" '@pnpm/already'
create_package "$test_tmp/wait" '@pnpm/wait'

cat > "$fake_bin/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
package_spec=$2
case "${NPM_STAGE_TEST_MODE:-stage}:$package_spec" in
  stage:@pnpm/already@1.0.0)
    echo 1.0.0
    ;;
  stage:@pnpm/staged@1.0.0|timeout:@pnpm/wait@1.0.0)
    echo 'npm error code E404' >&2
    exit 1
    ;;
  wait:@pnpm/wait@1.0.0)
    count_file="$NPM_STAGE_TEST_STATE_DIR/wait-count"
    count=0
    if [ -f "$count_file" ]; then
      count=$(<"$count_file")
    fi
    count=$((count + 1))
    echo "$count" > "$count_file"
    if [ "$count" -eq 1 ]; then
      echo 'npm error code E404' >&2
      exit 1
    fi
    echo 1.0.0
    ;;
  *)
    echo "unexpected npm view: $*" >&2
    exit 1
    ;;
esac
EOF
chmod +x "$fake_bin/npm"

cat > "$fake_bin/pnpm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "$NPM_STAGE_TEST_PNPM_LOG"
package_dir=${3%/}
name=$(jq -r '.publishConfig.name // .name' "$package_dir/package.json")
version=$(jq -r '.version' "$package_dir/package.json")
if [ "${NPM_STAGE_OUTPUT_MODE:-valid}" = missing-id ]; then
  jq -n --arg name "$name" --arg version "$version" \
    '{($name): {name: $name, version: $version}}'
else
  jq -n --arg name "$name" --arg version "$version" \
    '{($name): {name: $name, version: $version, stageId: "11111111-2222-4333-8444-555555555555"}}'
fi
EOF
chmod +x "$fake_bin/pnpm"

summary="$test_tmp/summary.md"
pnpm_log="$test_tmp/pnpm.log"
PATH="$fake_bin:$PATH" \
RUNNER_TEMP="$test_tmp" \
GITHUB_STEP_SUMMARY="$summary" \
NPM_STAGE_TEST_MODE=stage \
NPM_STAGE_TEST_PNPM_LOG="$pnpm_log" \
  "$script_dir/npm-staged-publication.sh" stage 'test layer' next-12 \
    "$test_tmp/staged" "$test_tmp/already"

grep -q 'stage publish.*/staged/ --tag next-12 --access public --provenance --no-git-checks --json' "$pnpm_log"
test "$(wc -l < "$pnpm_log")" -eq 1
grep -q '| `@pnpm/staged@1.0.0` | `11111111-2222-4333-8444-555555555555` |' "$summary"
grep -q '| `@pnpm/already@1.0.0` | already published |' "$summary"

if PATH="$fake_bin:$PATH" \
  RUNNER_TEMP="$test_tmp" \
  NPM_STAGE_TEST_MODE=stage \
  NPM_STAGE_OUTPUT_MODE=missing-id \
  NPM_STAGE_TEST_PNPM_LOG="$pnpm_log" \
    "$script_dir/npm-staged-publication.sh" stage 'test layer' next-12 \
      "$test_tmp/staged" > "$test_tmp/missing-id.out" 2>&1; then
  echo 'expected staging without a stage ID to fail' >&2
  exit 1
fi
grep -q 'returned no stage ID for @pnpm/staged@1.0.0' "$test_tmp/missing-id.out"

PATH="$fake_bin:$PATH" \
RUNNER_TEMP="$test_tmp" \
NPM_STAGE_TEST_MODE=wait \
NPM_STAGE_TEST_STATE_DIR="$state_dir" \
NPM_PUBLICATION_POLL_SECONDS=0 \
NPM_PUBLICATION_TIMEOUT_SECONDS=5 \
  "$script_dir/npm-staged-publication.sh" wait 'test layer' "$test_tmp/wait" \
    > "$test_tmp/wait.out"
grep -q 'Waiting for npm approval of the test layer packages' "$test_tmp/wait.out"
grep -q 'Every package in test layer is published' "$test_tmp/wait.out"

if PATH="$fake_bin:$PATH" \
  RUNNER_TEMP="$test_tmp" \
  NPM_STAGE_TEST_MODE=timeout \
  NPM_STAGE_TEST_STATE_DIR="$state_dir" \
  NPM_PUBLICATION_POLL_SECONDS=0 \
  NPM_PUBLICATION_TIMEOUT_SECONDS=0 \
    "$script_dir/npm-staged-publication.sh" wait 'test layer' "$test_tmp/wait" \
      > "$test_tmp/timeout.out" 2>&1; then
  echo 'expected publication wait to time out' >&2
  exit 1
fi
grep -q 'Timed out waiting for npm approval of the test layer packages' "$test_tmp/timeout.out"
