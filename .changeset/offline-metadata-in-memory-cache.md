---
"@pnpm/resolving.npm-resolver": patch
"pnpm": patch
---

Sped up offline and `--prefer-offline` resolution on large workspaces (e.g. `pnpm dedupe --offline`, `pnpm install --offline`). Package metadata loaded from the local cache is now kept in memory, so each package's metadata is parsed once per command instead of once per dependent that references it.
