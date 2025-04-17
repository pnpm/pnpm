---
"@pnpm/deps.status": minor
"pnpm": patch
---

Explicitly drop `verify-deps-before-run` support for `node-linker=pnp`. Combining `verify-deps-before-run` and `node-linker=pnp` will now print a warning.
