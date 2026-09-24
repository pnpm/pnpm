---
"@pnpm/installing.commands": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` no longer adds `allowBuilds` placeholder entries to `pnpm-workspace.yaml` when it runs in CI or without a terminal. Interactive installs still add them [#11574](https://github.com/pnpm/pnpm/issues/11574).
