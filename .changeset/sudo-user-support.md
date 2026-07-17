---
"pnpm": minor
"@pnpm/engine.pm.commands": minor
"@pnpm/config.reader": minor
---

Added support for `SUDO_USER`. When running `sudo pnpm install -g` or `sudo pnpm setup`, the global binaries and configuration changes will now be applied to the original user's home directory instead of the root directory.
