# Worktree helpers for bash/zsh.
# Add to your shell config:
#   source /path/to/pnpm/shell/wt.sh
#
# Usage:
#   wt <branch-name>   — create a worktree for a branch and switch to it
#   wt-pr <pr-number>  — create a worktree for a GitHub PR and switch to it
wt() {
  cd "$(pnpm worktree:new "$@" | tail -1)"
}

wt-pr() {
  cd "$(pnpm worktree:pr "$@" | tail -1)"
}
