---
"@pnpm/installing.commands": patch
"pnpm": patch
---

`pnpm link` with several paths now records every linked package in the `overrides` of `pnpm-workspace.yaml`. Some of them could be missing before.
