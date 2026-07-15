---
"@pnpm/cli.default-reporter": patch
"@pnpm/installing.commands": patch
"@pnpm/core-loggers": patch
"pnpm": patch
---

Keep the interactive `minimumReleaseAge` approval prompt visible during `pnpm install`. The progress reporter now pauses its redraws while a prompt is waiting for input instead of overwriting it, so the install no longer hangs on a question the user cannot see [#13019](https://github.com/pnpm/pnpm/issues/13019).
