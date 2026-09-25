#!/usr/bin/env bash
# Resolves merge conflicts for a GitHub PR by rebasing onto the latest base branch.
#
# Usage:
#   ./.agents/skills/pull-requests/scripts/resolve-pr-conflicts.sh <PR_NUMBER>            # full run
#   ./.agents/skills/pull-requests/scripts/resolve-pr-conflicts.sh <PR_NUMBER> --continue  # finish after manual resolution
#   ./.agents/skills/pull-requests/scripts/resolve-pr-conflicts.sh <PR_NUMBER> --no-push   # rebase only, push by hand
#
# Options:
#   --continue  Finish the rebase that was paused for manual conflict resolution. Only a
#               rebase this script started for that PR is continued: the PR it started for is
#               recorded when the rebase begins, and a mismatch is refused, not pushed.
#   --no-push   Stop after a successful rebase and print the push command instead of
#               running it. Resolve, review and publish are then three separate steps,
#               which is what a fork PR needs: the push goes to the contributor's branch.
#               Alias: --dry-run.
#
# Prerequisites:
# - gh CLI authenticated with access to pnpm/pnpm
# - "origin" remote must point to pnpm/pnpm (not a fork)
# - You must be on the PR's head branch (the script will checkout via gh if not)
#
# This script:
# 1. Checks out the PR branch if needed
# 2. Force-fetches the base branch to avoid stale refs
# 3. Rebases the current branch onto it
# 4. Auto-resolves pnpm-lock.yaml conflicts via lockfile-only install
# 5. For other conflicts, exits with the list of files needing manual resolution
# 6. After manual resolution, call with --continue to finish (rebase continue + push + verify)
# 7. Pass --no-push to stop after step 6's rebase, before the push

set -euo pipefail

usage() {
  echo "Usage: $0 <PR_NUMBER> [--continue] [--no-push]" >&2
}

PR_NUMBER=""
CONTINUE_MODE=""
NO_PUSH=0
for arg in "$@"; do
  case "$arg" in
    --continue) CONTINUE_MODE="--continue" ;;
    --no-push | --dry-run) NO_PUSH=1 ;;
    -h | --help)
      usage
      exit 0
      ;;
    -*)
      echo "ERROR: unknown option: $arg" >&2
      usage
      exit 2
      ;;
    *)
      if [ -n "$PR_NUMBER" ]; then
        echo "ERROR: unexpected argument: $arg" >&2
        usage
        exit 2
      fi
      PR_NUMBER="$arg"
      ;;
  esac
done
if [ -z "$PR_NUMBER" ]; then
  usage
  exit 2
fi
# The number is echoed back in the "run this next" lines and used to build API paths.
if [[ ! "$PR_NUMBER" =~ ^[0-9]+$ ]]; then
  echo "ERROR: PR_NUMBER must be a number, got: $PR_NUMBER" >&2
  usage
  exit 2
fi

REPO="pnpm/pnpm"

# A paused rebase leaves HEAD detached, so `git rev-parse --abbrev-ref HEAD` reports "HEAD"
# rather than the branch under rewrite.
rebase_in_progress() {
  [ -d "$(git rev-parse --git-path rebase-merge)" ] || [ -d "$(git rev-parse --git-path rebase-apply)" ]
}

# The branch a paused rebase is rewriting, without the "refs/heads/" prefix.
# Empty when git did not record it.
rebase_head_branch() {
  local dir name_file
  for dir in rebase-merge rebase-apply; do
    name_file="$(git rev-parse --git-path "$dir/head-name")"
    if [ -f "$name_file" ]; then
      sed -e 's|^refs/heads/||' -e '1q' "$name_file"
      return 0
    fi
  done
  echo ""
}

# The identity a rebase was started for, kept beside git's own rebase state. A branch name
# cannot say which PR a paused rebase belongs to — two forks can have the same head branch
# name — and --continue picks the remote to push to from the head owner.
rebase_state_file() {
  git rev-parse --git-path resolve-pr-conflicts.state
}

RECORDED_PR=""
RECORDED_OWNER=""
RECORDED_BRANCH=""

# Reads the identity recorded when the rebase started into RECORDED_PR, RECORDED_OWNER and
# RECORDED_BRANCH. Non-zero when there is no record, i.e. this script did not start the
# rebase that is in progress.
read_rebase_identity() {
  local state
  state="$(rebase_state_file)"
  if [ ! -f "$state" ]; then
    return 1
  fi
  RECORDED_PR="$(sed -n 's/^PR=//p' "$state" | sed -n '1p')"
  RECORDED_OWNER="$(sed -n 's/^OWNER=//p' "$state" | sed -n '1p')"
  RECORDED_BRANCH="$(sed -n 's/^BRANCH=//p' "$state" | sed -n '1p')"
}

record_rebase_identity() {
  printf 'PR=%s\nOWNER=%s\nBRANCH=%s\n' "$PR_NUMBER" "$HEAD_OWNER" "$HEAD_BRANCH" > "$(rebase_state_file)"
}

clear_rebase_identity() {
  rm -f "$(rebase_state_file)"
}

# Verify origin points to pnpm/pnpm (strict match for HTTPS or SSH)
ORIGIN_URL=$(git remote get-url origin 2>/dev/null || echo "")
if [[ ! "$ORIGIN_URL" =~ github\.com[:/]pnpm/pnpm(\.git)?$ ]]; then
  echo "ERROR: 'origin' remote does not point to pnpm/pnpm."
  echo "  Current origin: $ORIGIN_URL"
  echo "  Expected: https://github.com/pnpm/pnpm.git (or git@github.com:pnpm/pnpm.git)"
  exit 1
fi

# Get PR metadata
echo "Fetching PR #${PR_NUMBER} metadata..."
HEAD_OWNER=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json headRepositoryOwner --jq .headRepositoryOwner.login)
HEAD_BRANCH=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json headRefName --jq .headRefName)

# The paused rebase already knows the branch it is rewriting, so the checkout below is not
# just harmful there — it aborts on the resolutions staged for that rebase. Everything the
# rebase needs is in the work tree, and --continue is the only way forward from it.
if rebase_in_progress; then
  if [ "$CONTINUE_MODE" != "--continue" ]; then
    echo "ERROR: a rebase is already in progress."
    printf '  Finish it with: %q %s --continue\n' "$0" "$PR_NUMBER"
    exit 1
  fi

  REBASE_BRANCH="$(rebase_head_branch)"
  if [ -n "$REBASE_BRANCH" ] && [ "$REBASE_BRANCH" != "$HEAD_BRANCH" ]; then
    echo "ERROR: PR #$PR_NUMBER's head branch is '$HEAD_BRANCH', but the paused rebase is rewriting '$REBASE_BRANCH'."
    echo "  Refusing to continue: the rebased commits would be force-pushed to the wrong branch."
    exit 1
  fi

  if ! read_rebase_identity; then
    echo "ERROR: a rebase of '${REBASE_BRANCH:-the current branch}' is in progress, but PR #$PR_NUMBER's rebase did not start it."
    echo "  Refusing to continue: another fork's PR can use the same head branch name, and the"
    echo "  rebased commits would then be force-pushed to that PR's branch."
    echo "  Run 'git rebase --abort' to discard it, then re-run without --continue."
    exit 1
  fi
  if [ "$RECORDED_PR" != "$PR_NUMBER" ] ||
    [ "$RECORDED_OWNER" != "$HEAD_OWNER" ] ||
    [ "$RECORDED_BRANCH" != "$HEAD_BRANCH" ]; then
    echo "ERROR: the paused rebase belongs to PR #$RECORDED_PR ($RECORDED_OWNER:$RECORDED_BRANCH), not to PR #$PR_NUMBER ($HEAD_OWNER:$HEAD_BRANCH)."
    echo "  Refusing to continue: the rebased commits would be force-pushed to the wrong branch."
    exit 1
  fi
  echo "Rebase in progress on '$RECORDED_BRANCH' for PR #$PR_NUMBER; staying on the detached HEAD."
elif [ "$CONTINUE_MODE" = "--continue" ]; then
  echo "ERROR: --continue was passed, but no rebase is in progress."
  echo "  Run without --continue to rebase the PR branch onto the latest base branch."
  exit 1
else
  # Ensure we're on the PR branch (do this before determining push remote so gh can set up fork remotes)
  CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
  if [ "$CURRENT_BRANCH" != "$HEAD_BRANCH" ]; then
    echo "Not on PR branch ($CURRENT_BRANCH != $HEAD_BRANCH). Checking out via gh..."
    gh pr checkout "$PR_NUMBER"
    CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
    if [ "$CURRENT_BRANCH" != "$HEAD_BRANCH" ]; then
      echo "ERROR: Failed to checkout PR branch. Current branch: $CURRENT_BRANCH"
      exit 1
    fi
  fi
fi

# Determine push remote (after checkout, since gh pr checkout may add the fork remote)
REMOTE="origin"
if [ "$HEAD_OWNER" != "pnpm" ]; then
  if git remote get-url "$HEAD_OWNER" &>/dev/null; then
    REMOTE="$HEAD_OWNER"
  else
    # Try to auto-add the fork remote from the PR's clone URL
    FORK_URL="https://github.com/$HEAD_OWNER/pnpm.git"
    echo "Adding remote '$HEAD_OWNER' -> $FORK_URL"
    git remote add "$HEAD_OWNER" "$FORK_URL"
    REMOTE="$HEAD_OWNER"
  fi
fi

# Helper: regenerate lockfile without running lifecycle scripts
regenerate_lockfile() {
  echo "Regenerating pnpm-lock.yaml..."
  pnpm install --lockfile-only --no-frozen-lockfile --ignore-scripts
  git add pnpm-lock.yaml
}

# Publish the rebased branch and let GitHub re-evaluate mergeability.
finish_rebase() {
  clear_rebase_identity
  if [ "$NO_PUSH" -eq 1 ]; then
    echo ""
    echo "Rebase finished; not pushing (--no-push)."
    echo "Review the rewritten commits, then publish the branch with:"
    printf '  git push %q %q --force-with-lease\n' "$REMOTE" "HEAD:$HEAD_BRANCH"
    return 0
  fi

  echo "Force-pushing to $REMOTE/$HEAD_BRANCH..."
  git push "$REMOTE" "HEAD:$HEAD_BRANCH" --force-with-lease

  echo "Waiting for GitHub to update mergeability..."
  sleep 10
  MERGEABLE=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json mergeable --jq .mergeable)
  MERGE_STATE=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json mergeStateStatus --jq .mergeStateStatus)
  echo "PR status: mergeable=$MERGEABLE mergeStateStatus=$MERGE_STATE"

  if [ "$MERGEABLE" = "MERGEABLE" ]; then
    echo "Conflicts resolved successfully!"
  else
    echo "WARNING: GitHub still reports conflicts. Main may have moved again — re-run this script."
  fi
}

# --continue mode: finish a previously paused rebase, then push
if [ "$CONTINUE_MODE" = "--continue" ]; then
  echo "Continuing rebase..."

  # Regenerate lockfile if it was among the conflicted files
  if git diff --name-only --diff-filter=U 2>/dev/null | grep -q "pnpm-lock.yaml"; then
    echo "Auto-resolving pnpm-lock.yaml..."
    git checkout --ours pnpm-lock.yaml
    git add pnpm-lock.yaml
    regenerate_lockfile
  fi

  if ! GIT_EDITOR=true git rebase --continue; then
    echo "ERROR: 'git rebase --continue' failed. Resolve remaining conflicts and re-run with --continue."
    exit 1
  fi

  finish_rebase
  exit 0
fi

# Full mode: fetch, rebase, resolve

BASE_BRANCH=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json baseRefName --jq .baseRefName)
echo "Base: $BASE_BRANCH  Head: $HEAD_OWNER:$HEAD_BRANCH"

# Force-update the base branch ref (use + prefix to force non-fast-forward updates)
echo "Force-fetching origin/$BASE_BRANCH..."
git fetch origin "+refs/heads/$BASE_BRANCH:refs/remotes/origin/$BASE_BRANCH"

# Verify against GitHub
GITHUB_SHA=$(gh api "repos/$REPO/branches/$BASE_BRANCH" --jq '.commit.sha')
LOCAL_SHA=$(git rev-parse "origin/$BASE_BRANCH")
if [ "$GITHUB_SHA" != "$LOCAL_SHA" ]; then
  echo "ERROR: Local origin/$BASE_BRANCH ($LOCAL_SHA) doesn't match GitHub ($GITHUB_SHA)"
  exit 1
fi
echo "Base branch ref verified: $LOCAL_SHA"

# The rebase is recorded as belonging to this PR before it starts, so a pause can be tied
# back to it: --continue refuses a rebase that this script did not start.
record_rebase_identity

# Rebase
echo "Rebasing onto origin/$BASE_BRANCH..."
if git rebase "origin/$BASE_BRANCH"; then
  echo "Rebase completed cleanly."
else
  echo "Conflicts detected. Attempting auto-resolution..."

  CONFLICTED=$(git diff --name-only --diff-filter=U)
  MANUAL_FILES=()

  for file in $CONFLICTED; do
    if [ "$file" = "pnpm-lock.yaml" ]; then
      echo "  Auto-resolving pnpm-lock.yaml (will regenerate)..."
      git checkout --ours pnpm-lock.yaml
      git add pnpm-lock.yaml
    else
      MANUAL_FILES+=("$file")
    fi
  done

  if [ ${#MANUAL_FILES[@]} -gt 0 ]; then
    # Regenerate lockfile now if it was conflicted, before pausing
    if echo "$CONFLICTED" | grep -q "pnpm-lock.yaml"; then
      regenerate_lockfile
    fi

    echo ""
    echo "MANUAL_RESOLUTION_NEEDED"
    echo "The following files have conflicts that need manual resolution:"
    for f in "${MANUAL_FILES[@]}"; do
      echo "  $f"
    done
    echo ""
    echo "After resolving, stage the files with 'git add' and run:"
    printf '  %q %s --continue\n' "$0" "$PR_NUMBER"
    exit 1
  fi

  # All conflicts were auto-resolved — regenerate lockfile and continue
  if echo "$CONFLICTED" | grep -q "pnpm-lock.yaml"; then
    regenerate_lockfile
  fi

  if ! GIT_EDITOR=true git rebase --continue; then
    printf "ERROR: 'git rebase --continue' failed. Resolve remaining conflicts and run: %q %s --continue\n" "$0" "$PR_NUMBER"
    exit 1
  fi
fi

# Force push and verify, unless the caller asked to stop after the rebase
finish_rebase
