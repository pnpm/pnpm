---
"@pnpm/config.reader": patch
"pnpm": patch
---

`pnpm self-update` now ignores workspace settings that load executable hooks, including `configDependencies`, `pnpmfile`, and `globalPnpmfile`.
