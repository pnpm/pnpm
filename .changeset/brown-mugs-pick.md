---
"@pnpm/plugin-commands-deploy": patch
pnpm: patch
---

A [change in pnpm v10.6.3](https://github.com/pnpm/pnpm/pull/9259) introduced new bugs in `pnpm deploy` running in legacy mode. (Legacy deploys are performed when `force-legacy-deploy=true` or the `--legacy` flag is used.) This change is now reverted for `pnpm deploy` in legacy mode.
