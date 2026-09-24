#!/usr/bin/env bash

set -euo pipefail

script_dir=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
script="$script_dir/resolve-pr-conflicts.sh"
test_tmp=$(mktemp -d)
trap 'rm -rf "$test_tmp"' EXIT
fake_bin="$test_tmp/bin"
mkdir "$fake_bin"

git_dir="$test_tmp/git"
mkdir "$git_dir"
export STUB_GIT_DIR="$git_dir"
export STUB_HEAD_BRANCH='fix/example'
export STUB_CURRENT_BRANCH='fix/example'
export STUB_BASE_SHA='deadbeefdeadbeefdeadbeefdeadbeefdeadbeef'
export STUB_HEAD_OWNER='pnpm'

# The script drives git and gh through their real command lines, so the stubs answer
# from the state each case sets up and record every call for the assertions below.
cat > "$fake_bin/git" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'git %s\n' "$*" >> "$STUB_LOG"
case "$*" in
  'remote get-url origin') echo 'https://github.com/pnpm/pnpm.git' ;;
  'rev-parse --abbrev-ref HEAD')
    if [ -f "$STUB_GIT_DIR/checked-out" ]; then
      echo "$STUB_HEAD_BRANCH"
    else
      echo "$STUB_CURRENT_BRANCH"
    fi
    ;;
  'rev-parse --git-path rebase-merge') echo "$STUB_GIT_DIR/rebase-merge" ;;
  'rev-parse --git-path rebase-apply') echo "$STUB_GIT_DIR/rebase-apply" ;;
  'rev-parse --git-path rebase-merge/head-name') echo "$STUB_GIT_DIR/rebase-merge/head-name" ;;
  'rev-parse --git-path rebase-apply/head-name') echo "$STUB_GIT_DIR/rebase-apply/head-name" ;;
  'rev-parse origin/main') echo "$STUB_BASE_SHA" ;;
  *) ;;
esac
EOF
chmod +x "$fake_bin/git"

cat > "$fake_bin/gh" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'gh %s\n' "$*" >> "$STUB_LOG"
case "$*" in
  # This is what the real gh does while a rebase has staged resolutions in the work tree.
  *'pr checkout'*)
    if [ -d "$STUB_GIT_DIR/rebase-merge" ] || [ -d "$STUB_GIT_DIR/rebase-apply" ]; then
      echo 'error: Your local changes to the following files would be overwritten by checkout' >&2
      exit 1
    fi
    touch "$STUB_GIT_DIR/checked-out"
    ;;
  *'--json headRepositoryOwner'*) echo "$STUB_HEAD_OWNER" ;;
  *'--json headRefName'*) echo "$STUB_HEAD_BRANCH" ;;
  *'--json baseRefName'*) echo 'main' ;;
  *'--json mergeable'*) echo 'MERGEABLE' ;;
  *'--json mergeStateStatus'*) echo 'CLEAN' ;;
  *'api repos/pnpm/pnpm/branches/main'*) echo "$STUB_BASE_SHA" ;;
  *) ;;
esac
EOF
chmod +x "$fake_bin/gh"

cat > "$fake_bin/pnpm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf 'pnpm %s\n' "$*" >> "$STUB_LOG"
EOF
chmod +x "$fake_bin/pnpm"

# The real script waits for GitHub to recompute mergeability.
cat > "$fake_bin/sleep" <<'EOF'
#!/usr/bin/env bash
exit 0
EOF
chmod +x "$fake_bin/sleep"

start_rebase() {
  reset_state
  mkdir -p "$git_dir/rebase-merge"
  printf 'refs/heads/%s\n' "$1" > "$git_dir/rebase-merge/head-name"
}

# The apply backend writes rebase-apply instead of rebase-merge.
start_rebase_apply() {
  reset_state
  mkdir -p "$git_dir/rebase-apply"
  printf 'refs/heads/%s\n' "$1" > "$git_dir/rebase-apply/head-name"
}

reset_state() {
  rm -rf "$git_dir/rebase-merge" "$git_dir/rebase-apply" "$git_dir/checked-out"
}

no_rebase() {
  reset_state
}

# A paused rebase leaves HEAD detached, so `git rev-parse --abbrev-ref HEAD` reports "HEAD".
detached_head() {
  export STUB_CURRENT_BRANCH='HEAD'
}

on_pr_branch() {
  export STUB_CURRENT_BRANCH="$STUB_HEAD_BRANCH"
}

CURRENT_CASE=''
output=''
status=0

run_case() {
  CURRENT_CASE=$1
  shift
  STUB_LOG="$test_tmp/$CURRENT_CASE.log"
  : > "$STUB_LOG"
  export STUB_LOG
  set +e
  output=$(PATH="$fake_bin:$PATH" "$script" "$@" 2>&1)
  status=$?
  set -e
}

fail() {
  echo "FAIL ($CURRENT_CASE): $1" >&2
  echo "--- output ---" >&2
  printf '%s\n' "$output" >&2
  echo "--- calls ---" >&2
  cat "$STUB_LOG" >&2
  exit 1
}

expect_status() {
  [ "$status" -eq "$1" ] || fail "expected exit status $1, got $status"
}

expect_output() {
  printf '%s' "$output" | grep -q -- "$1" || fail "expected the output to contain: $1"
}

expect_call() {
  grep -q -- "$1" "$STUB_LOG" || fail "expected a call matching: $1"
}

expect_no_call() {
  if grep -q -- "$1" "$STUB_LOG"; then
    fail "expected no call matching: $1"
  fi
}

# --continue after a paused rebase: HEAD is detached, so the branch check must be skipped
# instead of running `gh pr checkout`, which aborts on the staged resolutions.
start_rebase 'fix/example'
detached_head
run_case 'continue-pushed' 4242 --continue
expect_status 0
expect_call '^git rebase --continue$'
expect_call '^git push origin HEAD:fix/example --force-with-lease$'
expect_output 'Conflicts resolved successfully!'
expect_no_call 'pr checkout'
expect_no_call 'git rev-parse --abbrev-ref HEAD'

# Same, for a rebase using the apply backend.
start_rebase_apply 'fix/example'
detached_head
run_case 'continue-apply-backend' 4242 --continue
expect_status 0
expect_call '^git rebase --continue$'
expect_no_call 'pr checkout'

# --no-push stops after the rebase and hands the push back to the caller.
start_rebase 'fix/example'
detached_head
run_case 'continue-no-push' 4242 --continue --no-push
expect_status 0
expect_call '^git rebase --continue$'
expect_no_call '^git push'
expect_output 'git push origin HEAD:fix/example --force-with-lease'

# --dry-run is documented as an alias of --no-push.
start_rebase 'fix/example'
detached_head
run_case 'continue-dry-run' 4242 --dry-run --continue
expect_status 0
expect_no_call '^git push'

# --continue with the wrong PR number must not force-push another branch's rebase.
start_rebase 'fix/somebody-else'
detached_head
run_case 'continue-wrong-branch' 4242 --continue
expect_status 1
expect_output "the paused rebase is rewriting 'fix/somebody-else'"
expect_no_call '^git rebase --continue$'
expect_no_call '^git push'

# A rebase already in progress without --continue: say so instead of attempting a nested rebase.
start_rebase 'fix/example'
detached_head
run_case 'rebase-without-continue' 4242
expect_status 1
expect_output 'a rebase is already in progress'
expect_no_call 'pr checkout'
expect_no_call '^git fetch'
expect_no_call '^git rebase origin/main'

# --continue without a rebase has nothing to continue.
no_rebase
detached_head
run_case 'continue-without-rebase' 4242 --continue
expect_status 1
expect_output 'no rebase is in progress'
expect_no_call '^git rebase --continue$'

# An unknown option is rejected before anything is touched.
no_rebase
on_pr_branch
run_case 'unknown-option' 4242 --frobnicate
expect_status 2
expect_output 'unknown option: --frobnicate'
expect_no_call '^git '

# A full run still rebases and pushes.
no_rebase
on_pr_branch
run_case 'full-pushed' 4242
expect_status 0
expect_call '^git rebase origin/main$'
expect_call '^git push origin HEAD:fix/example --force-with-lease$'

# A full run with --no-push leaves the branch unpublished.
no_rebase
on_pr_branch
run_case 'full-no-push' 4242 --no-push
expect_status 0
expect_call '^git rebase origin/main$'
expect_no_call '^git push'
expect_output 'git push origin HEAD:fix/example --force-with-lease'

# A full run on another branch still checks the PR branch out.
no_rebase
export STUB_CURRENT_BRANCH='fix/other'
run_case 'full-checkout' 4242 --no-push
expect_status 0
expect_call 'pr checkout 4242'

echo 'resolve-pr-conflicts.sh: all cases passed'
