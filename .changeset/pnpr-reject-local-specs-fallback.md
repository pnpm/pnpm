---
"@pnpm/installing.deps-installer": patch
"@pnpm/pnpr": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` with `pnprServer` configured now falls back to local resolution when the project declares `file:`, `link:`, or local path dependencies. `pnpr` also rejects local filesystem dependency specifiers on `/-/pnpr/v0/resolve`.
