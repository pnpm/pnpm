---
"@pnpm/deps.status": patch
"pnpm": patch
"pacquet": patch
---

`pnpm install` now takes its fast path in a workspace with `patchedDependencies` when the previous install ran with another major version of pnpm.
