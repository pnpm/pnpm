---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when `allowUnusedPatches` in `pnpm-workspace.yaml` is not a boolean. A quoted value such as `"false"` was treated as `true`.
