---
"@pnpm/installing.commands": patch
"@pnpm/releasing.commands": patch
"pnpm": patch
---

`pnpm deploy --legacy` no longer rewrites `node_modules/.pnpm-workspace-state-v1.json` in the source workspace. The next `verifyDepsBeforeRun` check there reported the workspace as out of date [#15352](https://github.com/pnpm/pnpm/issues/15352).
