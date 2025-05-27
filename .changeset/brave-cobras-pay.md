---
"@pnpm/plugin-commands-deploy": patch
"pnpm": patch
---

Fix an issue in which `pnpm deploy --legacy` creates unexpected directories when `pnpm-workspace.yaml` exist [#9550](https://github.com/pnpm/pnpm/issues/9550).
