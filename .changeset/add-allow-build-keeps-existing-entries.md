---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm add --allow-build` now adds to the `allowBuilds` entries already in `pnpm-workspace.yaml` instead of replacing them [#13872](https://github.com/pnpm/pnpm/issues/13872).
