---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

Fix a bug in which `pnpm deploy` fails to read the correct `projectId` when the deploy source is the same as the workspace directory [#9001](https://github.com/pnpm/pnpm/issues/9001).
