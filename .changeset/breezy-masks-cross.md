---
"@pnpm/plugin-commands-setup": patch
"pnpm": patch
---

The configuration added by `pnpm setup` should check if the pnpm home directory is already in the PATH before adding to the PATH.

Before this change, this code was added to the shell:

```sh
export PNPM_HOME="$HOME/Library/pnpm"
export PATH="$PNPM_HOME:$PATH"
```

Now this will be added:

```sh
export PNPM_HOME="$HOME/Library/pnpm"
case ":$PATH:" in
  *":$PNPM_HOME:"*) ;;
  *) export PATH="$PNPM_HOME:$PATH" ;;
esac
```
