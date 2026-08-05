---
"pnpm": minor
---

Running `pnpm setup`, `pnpm self-update`, or a command that modifies the global installation (such as `pnpm add --global`) through `sudo` now prints a warning. pnpm keeps global packages and configuration in the invoking user's home directory, so running these commands as root silently operates on the root user's home directory instead of yours. They will fail with `ERR_PNPM_SUDO_NOT_SUPPORTED` in pnpm v12. Read-only global commands (such as `pnpm bin --global`) are unaffected.
