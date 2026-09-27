---
"@pnpm/workspace.root-finder": patch
"pnpm": patch
"pacquet": patch
---

pnpm now reads the workspace directory override from `PNPM_CONFIG_WORKSPACE_DIR`, like other settings. `NPM_CONFIG_WORKSPACE_DIR` still works as a fallback [#16275](https://github.com/pnpm/pnpm/issues/16275).
