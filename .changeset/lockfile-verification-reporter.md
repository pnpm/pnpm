---
"@pnpm/cli.default-reporter": patch
"@pnpm/core-loggers": patch
"pnpm": patch
---

When an install is delegated to the Rust engine, pnpm now renders the lockfile verification progress that the engine reports. The pass and failure lines show how many entries were checked.
