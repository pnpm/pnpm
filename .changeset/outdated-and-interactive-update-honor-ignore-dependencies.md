---
"pacquet": patch
---

`pnpm outdated` and `pnpm update --interactive` now leave out the dependencies listed in `updateConfig.ignoreDependencies`, instead of reporting them and offering them for update.
