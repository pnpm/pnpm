# Worktree helper for fish.
# Add to your shell config:
#   source /path/to/pnpm/wt.fish
#
# Usage: wt <branch-name>
function wt
    cd (pnpm worktree:new $argv | tail -1)
end
