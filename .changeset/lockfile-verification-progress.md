---
"@pnpm/cli.default-reporter": patch
"@pnpm/core-loggers": patch
"pacquet": patch
"pnpm": patch
---

`pnpm install` now shows lockfile verification progress while the supply-chain check runs. The final pass or failure line reports how many entries were checked.
