---
"@pnpm/config.reader": patch
"pnpm": patch
---

pnpm now reports an `INVALID_SETTING` error when `scriptShell` or `pnprServer` in `pnpm-workspace.yaml` is not a string.
