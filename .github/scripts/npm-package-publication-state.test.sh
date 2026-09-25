#!/usr/bin/env bash

# cspell:ignore ECONNRESET

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
test_tmp=$(mktemp -d)
trap 'rm -rf "$test_tmp"' EXIT
fake_bin="$test_tmp/bin"
mkdir "$fake_bin"

cat > "$fake_bin/npm" <<'EOF'
#!/usr/bin/env bash
if [ "$4" != '--registry' ] || [ "$5" != 'https://registry.npmjs.org/' ]; then
  echo "unexpected registry arguments: $*" >&2
  exit 1
fi
case "$2" in
  published)
    echo 1.0.0
    ;;
  missing)
    echo 'npm error code E404' >&2
    exit 1
    ;;
  missing-legacy)
    echo 'npm ERR! code E404' >&2
    exit 1
    ;;
  network)
    echo 'npm error code ECONNRESET' >&2
    exit 1
    ;;
  auth)
    echo 'npm error code E403' >&2
    exit 1
    ;;
  server)
    echo 'npm error code E500' >&2
    exit 1
    ;;
  misleading-e404)
    printf 'npm error code E403\nproxy response mentioned E404\n::warning::forged\n' >&2
    exit 1
    ;;
esac
EOF
chmod +x "$fake_bin/npm"

assert_state() {
  local expected=$1
  local package_spec=$2
  local actual
  actual=$(PATH="$fake_bin:$PATH" RUNNER_TEMP="$test_tmp" "$script_dir/npm-package-publication-state.sh" "$package_spec")
  test "$actual" = "$expected"
}

assert_failure() {
  local package_spec=$1
  local output
  if output=$(PATH="$fake_bin:$PATH" RUNNER_TEMP="$test_tmp" "$script_dir/npm-package-publication-state.sh" "$package_spec" 2>&1); then
    echo "expected npm view failure for $package_spec" >&2
    exit 1
  fi
  grep -q '^::error::npm view failed$' <<< "$output"
  if tail -n +2 <<< "$output" | grep -q '^::'; then
    echo "npm output produced a workflow command for $package_spec" >&2
    exit 1
  fi
}

assert_state published published
assert_state missing missing
assert_state missing missing-legacy
assert_failure network
assert_failure auth
assert_failure server
assert_failure misleading-e404
