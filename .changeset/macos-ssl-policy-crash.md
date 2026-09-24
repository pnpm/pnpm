---
"pacquet": patch
"@pnpm/pnpr": patch
---

`pnpm` no longer crashes on macOS when macOS cannot create an SSL policy for a registry connection. The client falls back to the bundled certificate roots [pnpm/pnpm#14461](https://github.com/pnpm/pnpm/issues/14461).
