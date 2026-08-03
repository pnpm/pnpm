#!/usr/bin/env bash
# Resolves merge conflicts for a GitHub PR by rebasing onto the latest base branch.
#
# Usage:
#   ./shell/resolve-pr-conflicts.sh <PR_NUMBER>                 # full run
#   ./shell/resolve-pr-conflicts.sh <PR_NUMBER> --continue       # finish after manual resolution
#   ./shell/resolve-pr-conflicts.sh <PR_NUMBER> --continue --no-push
#   ./shell/resolve-pr-conflicts.sh <PR_NUMBER> --no-push        # resolve locally without force-push
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
# 6. After manual resolution, call with --continue to finish (rebase continue + optional push + verify)

set -euo pipefail

PR_NUMBER="${1:?Usage: $0 <PR_NUMBER> [--continue] [--no-push]}"
shift || true

CONTINUE_MODE=0
NO_PUSH=0
for arg in "$@"; do
  case "$arg" in
    --continue) CONTINUE_MODE=1 ;;
    --no-push) NO_PUSH=1 ;;
    *)
      echo "Unknown argument: $arg"
      echo "Usage: $0 <PR_NUMBER> [--continue] [--no-push]"
      exit 1
      ;;
  esac
done

REPO="pnpm/pnpm"

in_rebase () {
  git rev-parse --git-path rebase-merge >/dev/null 2>&1 && [ -d "$(git rev-parse --git-path rebase-merge)" ] && return 0
  git rev-parse --git-path rebase-apply >/dev/null 2>&1 && [ -d "$(git rev-parse --git-path rebase-apply)" ] && return 0
  return 1
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

# --continue must run before any checkout: during a paused rebase HEAD is
# detached, so the branch check would call `gh pr checkout` and wipe staged
# conflict resolutions (https://github.com/pnpm/pnpm/issues/13106).
if [ "$CONTINUE_MODE" -eq 1 ]; then
  if ! in_rebase; then
    echo "ERROR: --continue requires an in-progress rebase (rebase-merge / rebase-apply)."
    exit 1
  fi
fi

# Ensure we're on the PR branch (skip while a rebase is already in progress)
CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD 2>/dev/null || echo "")
if ! in_rebase && [ "$CURRENT_BRANCH" != "$HEAD_BRANCH" ]; then
  echo "Not on PR branch ($CURRENT_BRANCH != $HEAD_BRANCH). Checking out via gh..."
  gh pr checkout "$PR_NUMBER"
  CURRENT_BRANCH=$(git rev-parse --abbrev-ref HEAD)
  if [ "$CURRENT_BRANCH" != "$HEAD_BRANCH" ]; then
    echo "ERROR: Failed to checkout PR branch. Current branch: $CURRENT_BRANCH"
    exit 1
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

push_and_verify() {
  if [ "$NO_PUSH" -eq 1 ]; then
    echo "Skipping push (--no-push). Rebase finished locally on $HEAD_BRANCH."
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

# --continue mode: finish a previously paused rebase, then optionally push
if [ "$CONTINUE_MODE" -eq 1 ]; then
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

  push_and_verify
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
    echo "  $0 $PR_NUMBER --continue"
    echo "Or resolve without pushing:"
    echo "  $0 $PR_NUMBER --continue --no-push"
    exit 1
  fi

  # All conflicts were auto-resolved — regenerate lockfile and continue
  if echo "$CONFLICTED" | grep -q "pnpm-lock.yaml"; then
    regenerate_lockfile
  fi

  if ! GIT_EDITOR=true git rebase --continue; then
    echo "ERROR: 'git rebase --continue' failed. Resolve remaining conflicts and run: $0 $PR_NUMBER --continue"
    exit 1
  fi
fi

push_and_verify
