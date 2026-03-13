#!/usr/bin/env bash
# Removes git worktrees whose branches are associated with merged PRs.
# Usage: ./cleanup-worktrees.sh [--dry-run]
#
# Requires: gh (GitHub CLI), git

set -euo pipefail

DRY_RUN=false
if [[ "${1:-}" == "--dry-run" ]]; then
  DRY_RUN=true
  echo "=== DRY RUN MODE ==="
  echo
fi

# Get the GitHub repo (owner/name) from the origin remote
GH_REPO=$(gh repo view --json nameWithOwner --jq '.nameWithOwner' 2>/dev/null)
if [[ -z "$GH_REPO" ]]; then
  echo "Error: Could not determine GitHub repository. Make sure 'gh' is authenticated." >&2
  exit 1
fi
echo "Repository: $GH_REPO"
echo

# Get the current worktree path so we don't remove it
CURRENT_WORKTREE="$(pwd -P)"

removed=0
skipped=0

while IFS= read -r line; do
  [[ -z "$line" ]] && continue

  worktree_path=$(echo "$line" | awk '{print $1}')
  branch=$(echo "$line" | awk '{print $3}' | tr -d '[]')

  # Skip detached HEAD entries
  [[ "$branch" == "detached" ]] && continue

  # Skip bare repo root (shown as "(bare)")
  echo "$line" | grep -q '(bare)$' && continue

  # Skip protected long-lived branches
  case "$branch" in
    main|master|v[0-9]*)
      echo "SKIP (protected branch): $worktree_path [$branch]"
      skipped=$((skipped + 1))
      continue
      ;;
  esac

  # Skip the current worktree
  real_wt="$(cd "$worktree_path" 2>/dev/null && pwd -P)" || continue
  if [[ "$real_wt" == "$CURRENT_WORKTREE" ]]; then
    echo "SKIP (current worktree): $worktree_path [$branch]"
    skipped=$((skipped + 1))
    continue
  fi

  # Look for merged PRs with this branch as the head
  merged_pr=$(gh pr list \
    --repo "$GH_REPO" \
    --head "$branch" \
    --state merged \
    --json number,title \
    --jq 'if length > 0 then .[0] | "\(.number)\t\(.title)" else "" end' \
    2>/dev/null || true)

  if [[ -n "$merged_pr" ]]; then
    pr_number=$(echo "$merged_pr" | cut -f1)
    pr_title=$(echo "$merged_pr" | cut -f2-)
    echo "MERGED: $worktree_path"
    echo "  Branch: $branch"
    echo "  PR #$pr_number: $pr_title"

    if [[ "$DRY_RUN" == false ]]; then
      git worktree remove --force "$worktree_path" && \
        echo "  -> Removed worktree" || \
        echo "  -> Failed to remove worktree"
      # Also delete the branch
      git branch -D "$branch" 2>/dev/null && \
        echo "  -> Deleted branch $branch" || true
    else
      echo "  -> Would remove worktree and delete branch"
    fi
    echo
    removed=$((removed + 1))
  else
    echo "SKIP (no merged PR): $worktree_path [$branch]"
    skipped=$((skipped + 1))
  fi
done < <(git worktree list)

echo
echo "---"
if [[ "$DRY_RUN" == true ]]; then
  echo "Would remove $removed worktree(s). Skipped $skipped."
  echo "Run without --dry-run to actually remove them."
else
  echo "Removed $removed worktree(s). Skipped $skipped."
fi
