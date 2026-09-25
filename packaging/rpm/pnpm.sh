# Installed as /etc/profile.d/pnpm.sh — the equivalent of the shell-profile
# block get.pnpm.io/install.sh appends: `pnpm add -g` puts binaries in
# PNPM_HOME, and pnpm refuses to install globally while it is unset.
export PNPM_HOME="${PNPM_HOME:-$HOME/.local/share/pnpm}"
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac
