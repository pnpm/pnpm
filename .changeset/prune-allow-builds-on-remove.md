---
"@pnpm/workspace.workspace-manifest-writer": patch
"@pnpm/installing.commands": patch
"pacquet": patch
"pnpm": patch
---

`pnpm remove` now prunes undecided entries (`"set this to true or false"`) from `allowBuilds` in `pnpm-workspace.yaml` when the corresponding packages are removed [pnpm/pnpm#13892](https://github.com/pnpm/pnpm/issues/13892).
