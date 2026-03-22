# Worktree helper for fish.
# Add to your shell config:
#   source /path/to/pnpm/shell/wt.fish
#
# Usage:
#   wt <branch-name>  — create a worktree for a branch and switch to it
#   wt <pr-number>    — create a worktree for a GitHub PR and switch to it
function wt
    set -l dir (pnpm worktree:new $argv | tail -1)
    or return 1
    test -n "$dir" -a -d "$dir"; or return 1
    cd $dir

    # If the argument looks like a PR number, auto-start Claude to review it
    if test (count $argv) -ge 1; and string match -qr '^\d+$' -- $argv[1]
        set -l pr_number $argv[1]
        claude --dangerously-skip-permissions "Review and fix PR #$pr_number. Steps:
1. Use gh to read the PR description, diff, and all review comments (both PR-level and inline).
2. Understand the intent of the PR and what each change does.
3. Resolve any conflicts with the base branch: use 'gh pr view $pr_number --json baseRefName' to get the base branch name, then force-fetch it with 'git fetch origin refs/heads/<base>:refs/remotes/origin/<base>' (plain 'git fetch' can return stale refs). Verify the SHA matches 'gh api repos/pnpm/pnpm/branches/<base> --jq .commit.sha'. Then rebase with 'git rebase origin/<base>' (prefer rebase over merge — merge commits can cause GitHub to still report conflicts). For lockfile conflicts: 'git checkout --ours pnpm-lock.yaml && git add pnpm-lock.yaml && pnpm install --no-frozen-lockfile && git add pnpm-lock.yaml'. Note: during rebase --ours=base branch, --theirs=your commit (opposite of merge). Do NOT skip this step. Do NOT assume the branch is up to date.
4. Address every review comment — fix the code as requested or as appropriate.
5. Look for any other bugs, issues, or style problems in the changed code and fix those too.
6. Run the relevant tests to verify your fixes work (check CLAUDE.md for how to run tests).
7. Give me a summary of what you found and what you changed, including any conflicts you resolved.
Do NOT push. Leave all non-merge changes unstaged for me to review."
    end
end
