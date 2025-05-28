---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

Fix an issue in which `pnpm deploy --legacy` creates unexpected directories when the root `package.json` has a workspace package as a peer dependency [#9550](https://github.com/pnpm/pnpm/issues/9550).
