---
"@pnpm/plugin-commands-config": patch
"@pnpm/run-npm": patch
---

Reverted change related to setting explicitly the npm config file path, which caused regressions.
