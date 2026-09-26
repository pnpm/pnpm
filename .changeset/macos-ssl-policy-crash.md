---
"pacquet": patch
"@pnpm/pnpr": patch
---

On macOS, `pnpm` now uses its bundled certificate roots when macOS cannot create an SSL policy for a registry connection. It crashed on the first registry request in that case [pnpm/pnpm#14461](https://github.com/pnpm/pnpm/issues/14461).
