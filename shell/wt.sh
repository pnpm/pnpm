# Worktree helper for bash/zsh.
# Add to your shell config:
#   source /path/to/pnpm/wt.sh
#
# Usage: wt <branch-name>
wt() {
  cd "$(pnpm worktree:new "$@" | tail -1)"
}
