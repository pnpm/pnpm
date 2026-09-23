---
"pacquet": patch
---

On Windows, `pnpm clean` and installs no longer fail immediately when another process uses a package in `node_modules`. pnpm waits up to a minute for an open file. It waits up to 5 seconds for a running program [pnpm/pnpm#15081](https://github.com/pnpm/pnpm/issues/15081).
