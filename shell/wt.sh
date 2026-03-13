# Worktree helper for bash/zsh.
# Add to your shell config:
#   source /path/to/pnpm/shell/wt.sh
#
# Usage:
#   wt <branch-name>  — create a worktree for a branch and switch to it
#   wt <pr-number>    — create a worktree for a GitHub PR and switch to it
wt() {
  cd "$(pnpm worktree:new "$@" | tail -1)"
}
