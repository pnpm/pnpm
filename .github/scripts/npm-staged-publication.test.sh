#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
test_tmp=$(mktemp -d)
trap 'rm -rf "$test_tmp"' EXIT
fake_bin="$test_tmp/bin"
mkdir "$fake_bin"

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

cat > "$fake_bin/npm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
if [ "$4" != '--registry' ] || [ "$5" != 'https://registry.npmjs.org/' ]; then
  echo "unexpected registry arguments: $*" >&2
  exit 1
fi
package_spec=$2
case "${NPM_STAGE_TEST_MODE:-stage}:$package_spec" in
  stage:@pnpm/already@1.0.0)
    echo 1.0.0
    ;;
  stage:@pnpm/staged@1.0.0)
    echo 'npm error code E404' >&2
    exit 1
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
  echo '::warning::forged-stage-output'
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

grep -q 'stage publish.*/staged/ --registry https://registry.npmjs.org/ --npmrc-auth-file /dev/null --tag next-12 --access public --provenance --no-git-checks --reporter=silent --json' "$pnpm_log"
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
grep -q '^stage output: ::warning::forged-stage-output$' "$test_tmp/missing-id.out"
if grep -q '^::warning::forged-stage-output$' "$test_tmp/missing-id.out"; then
  echo 'stage output produced a workflow command' >&2
  exit 1
fi
