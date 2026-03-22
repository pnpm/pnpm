#!/usr/bin/env bash
# Resolves merge conflicts for a GitHub PR by rebasing onto the latest base branch.
#
# Usage: ./shell/resolve-pr-conflicts.sh <PR_NUMBER>
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
# 4. If there are lockfile conflicts, auto-resolves them via pnpm install
# 5. Force-pushes the result
# 6. Verifies GitHub sees the PR as mergeable

set -euo pipefail

PR_NUMBER="${1:?Usage: $0 <PR_NUMBER>}"
REPO="pnpm/pnpm"

# Verify origin points to pnpm/pnpm
ORIGIN_URL=$(git remote get-url origin 2>/dev/null || echo "")
if [[ ! "$ORIGIN_URL" =~ pnpm/pnpm ]]; then
  echo "ERROR: 'origin' remote does not point to pnpm/pnpm."
  echo "  Current origin: $ORIGIN_URL"
  echo "  Expected: https://github.com/pnpm/pnpm.git (or SSH equivalent)"
  exit 1
fi

# Get PR metadata
echo "Fetching PR #${PR_NUMBER} metadata..."
BASE_BRANCH=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json baseRefName --jq .baseRefName)
HEAD_BRANCH=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json headRefName --jq .headRefName)
HEAD_OWNER=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json headRepositoryOwner --jq .headRepositoryOwner.login)

echo "Base: $BASE_BRANCH  Head: $HEAD_OWNER:$HEAD_BRANCH"

# Ensure we're on the PR branch
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

# Step 1: Force-update the base branch ref
echo "Force-fetching origin/$BASE_BRANCH..."
git fetch origin "refs/heads/$BASE_BRANCH:refs/remotes/origin/$BASE_BRANCH"

# Verify against GitHub
GITHUB_SHA=$(gh api "repos/$REPO/branches/$BASE_BRANCH" --jq '.commit.sha')
LOCAL_SHA=$(git rev-parse "origin/$BASE_BRANCH")
if [ "$GITHUB_SHA" != "$LOCAL_SHA" ]; then
  echo "ERROR: Local origin/$BASE_BRANCH ($LOCAL_SHA) doesn't match GitHub ($GITHUB_SHA)"
  echo "Try running: git fetch origin refs/heads/$BASE_BRANCH:refs/remotes/origin/$BASE_BRANCH"
  exit 1
fi
echo "Base branch ref verified: $LOCAL_SHA"

# Step 2: Rebase
echo "Rebasing onto origin/$BASE_BRANCH..."
if git rebase "origin/$BASE_BRANCH"; then
  echo "Rebase completed cleanly."
else
  echo "Conflicts detected. Attempting auto-resolution..."

  # Check for lockfile conflicts
  CONFLICTED=$(git diff --name-only --diff-filter=U)
  NEEDS_MANUAL=false

  for file in $CONFLICTED; do
    if [ "$file" = "pnpm-lock.yaml" ]; then
      echo "  Auto-resolving pnpm-lock.yaml (will regenerate)..."
      git checkout --ours pnpm-lock.yaml
      git add pnpm-lock.yaml
    else
      echo "  MANUAL resolution needed: $file"
      NEEDS_MANUAL=true
    fi
  done

  if [ "$NEEDS_MANUAL" = true ]; then
    echo ""
    echo "Some conflicts require manual resolution. Resolve them, then run:"
    echo "  git add <resolved files>"
    echo "  pnpm install --no-frozen-lockfile  # if lockfile was conflicted"
    echo "  git add pnpm-lock.yaml"
    echo "  git rebase --continue"
    echo "  $0 $PR_NUMBER  # re-run this script to push"
    exit 1
  fi

  # Regenerate lockfile if it was conflicted
  if echo "$CONFLICTED" | grep -q "pnpm-lock.yaml"; then
    echo "Regenerating pnpm-lock.yaml..."
    pnpm install --no-frozen-lockfile
    git add pnpm-lock.yaml
  fi

  if ! GIT_EDITOR=true git rebase --continue; then
    echo "ERROR: 'git rebase --continue' failed."
    echo "The rebase is not complete. Please resolve any remaining conflicts"
    echo "and run 'git rebase --continue' manually, then re-run this script to push."
    exit 1
  fi
fi

# Step 3: Force push
REMOTE="origin"
if [ "$HEAD_OWNER" != "pnpm" ]; then
  # PR is from a fork — check if we have that remote
  if git remote get-url "$HEAD_OWNER" &>/dev/null; then
    REMOTE="$HEAD_OWNER"
  else
    echo "Remote '$HEAD_OWNER' not found. Add it with:"
    echo "  git remote add $HEAD_OWNER https://github.com/$HEAD_OWNER/pnpm.git"
    exit 1
  fi
fi

echo "Force-pushing to $REMOTE/$HEAD_BRANCH..."
git push "$REMOTE" "HEAD:$HEAD_BRANCH" --force-with-lease

# Step 4: Verify
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
