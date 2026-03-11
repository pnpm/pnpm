# Worktree helper for fish.
# Add to your shell config:
#   source /path/to/pnpm/shell/wt.fish
#
# Usage:
#   wt <branch-name>  — create a worktree for a branch and switch to it
#   wt <pr-number>    — create a worktree for a GitHub PR and switch to it
function wt
    cd (pnpm worktree:new $argv | tail -1)
end
