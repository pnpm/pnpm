#!/usr/bin/env bash
# Resolves merge conflicts for a GitHub PR by rebasing onto the latest base branch.
#
# Usage: ./shell/resolve-pr-conflicts.sh <PR_NUMBER>
#
# This script:
# 1. Force-fetches the base branch to avoid stale refs
# 2. Rebases the current branch onto it
# 3. If there are lockfile conflicts, auto-resolves them via pnpm install
# 4. Force-pushes the result
# 5. Verifies GitHub sees the PR as mergeable

set -euo pipefail

PR_NUMBER="${1:?Usage: $0 <PR_NUMBER>}"
REPO="pnpm/pnpm"

# Get PR metadata
echo "Fetching PR #${PR_NUMBER} metadata..."
PR_JSON=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json baseRefName,headRefName,headRepository,headRepositoryOwner)
BASE_BRANCH=$(echo "$PR_JSON" | jq -r .baseRefName)
HEAD_BRANCH=$(echo "$PR_JSON" | jq -r .headRefName)
HEAD_OWNER=$(echo "$PR_JSON" | jq -r .headRepositoryOwner.login)

echo "Base: $BASE_BRANCH  Head: $HEAD_OWNER:$HEAD_BRANCH"

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

  GIT_EDITOR=true git rebase --continue || true
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
MERGE_STATUS=$(gh pr view "$PR_NUMBER" --repo "$REPO" --json mergeable,mergeStateStatus --jq '.')
echo "PR status: $MERGE_STATUS"

MERGEABLE=$(echo "$MERGE_STATUS" | jq -r .mergeable)
if [ "$MERGEABLE" = "MERGEABLE" ]; then
  echo "Conflicts resolved successfully!"
else
  echo "WARNING: GitHub still reports conflicts. Main may have moved again — re-run this script."
fi
