---
"@pnpm/installing.deps-installer": patch
"@pnpm/cli.default-reporter": patch
"@pnpm/core-loggers": patch
"pnpm": patch
---

Improve lockfile supply-chain verification logging by reporting checked progress (`x/y` entries) during verification and in final success/failure messages.