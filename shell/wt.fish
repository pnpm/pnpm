# Worktree helper for fish.
# Add to your shell config:
#   source /path/to/pnpm/shell/wt.fish
#
# Usage:
#   wt <branch-name>  — create a worktree for a branch and switch to it
#   wt <pr-number>    — create a worktree for a GitHub PR and switch to it
function wt
    set -l dir (pnpm worktree:new $argv | tail -1)
    cd $dir

    # If the argument looks like a PR number, auto-start Claude to review it
    if string match -qr '^\d+$' -- $argv[1]
        set -l pr_number $argv[1]
        claude --dangerously-skip-permissions --resume no --init-prompt "Review and fix PR #$pr_number. Steps:
1. Use gh to read the PR description, diff, and all review comments (both PR-level and inline).
2. Understand the intent of the PR and what each change does.
3. Resolve any conflicts with the base branch: use 'gh pr view $pr_number --json baseRefName' to get the base branch name, then run 'git fetch origin <base> && git merge origin/<base>'. If there are merge conflicts, resolve them. Do NOT skip this step. Do NOT assume the branch is up to date — always fetch and merge to be sure.
4. Address every review comment — fix the code as requested or as appropriate.
5. Look for any other bugs, issues, or style problems in the changed code and fix those too.
6. Run the relevant tests to verify your fixes work (check CLAUDE.md for how to run tests).
7. Give me a summary of what you found and what you changed, including any conflicts you resolved.
Do NOT push. Leave all non-merge changes unstaged for me to review."
    end
end
