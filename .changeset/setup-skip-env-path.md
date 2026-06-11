---
"@pnpm/engine.pm.commands": minor
"pnpm": minor
---

Added a `--skip-env-path` option to `pnpm setup` that skips updating shell configuration files or the Windows registry. When set, pnpm prints the values that should be added to the environment instead [#11500](https://github.com/pnpm/pnpm/issues/11500).
