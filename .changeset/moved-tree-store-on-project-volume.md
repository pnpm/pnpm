---
"pacquet": patch
---

`pnpm run` and `pnpm exec` with `verifyDepsBeforeRun` now accept a moved project whose store is on the project's volume. Before, the check reported that the workspace structure had changed whenever the default store was not on the home volume.
