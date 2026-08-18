---
"pacquet": minor
---

Running `pnpm setup`, `pnpm self-update`, or a command that modifies the global installation (such as `pnpm add --global`) through `sudo` now fails with `ERR_PNPM_SUDO_NOT_SUPPORTED` instead of silently operating on the root user's home directory. pnpm keeps global packages and configuration in the invoking user's home directory, so these commands never need root permissions. Read-only global commands (such as `pnpm bin --global`) still work under sudo.
